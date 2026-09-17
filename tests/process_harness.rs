//! Exercises the shared CLI runtime through real child-process stdout/stderr pipe boundaries.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

fn fixture() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cli_runtime_fixture"));
    for key in [
        "NO_COLOR",
        "CLICOLOR_FORCE",
        "FORCE_COLOR",
        "ORES_CLIS_SIGNAL_HANDLERS",
        "ORES_CLIS_SIGNAL_TTY_REQUIREMENT",
    ] {
        command.env_remove(key);
    }
    command
}

#[test]
fn redirected_stdout_uses_machine_output_without_ansi() {
    let output = fixture().arg("result").output().expect("fixture output");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"{\"ok\":true}\n");
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(&0x1b));
}

#[test]
fn explicit_human_color_can_be_forced_across_a_pipe() {
    let output = fixture()
        .args(["--output=human", "--color=always", "result"])
        .output()
        .expect("fixture output");
    assert!(output.status.success());
    assert!(output.stdout.contains(&0x1b));
    assert!(String::from_utf8_lossy(&output.stdout).contains("ok"));
}

#[test]
fn json_primary_stream_stays_plain_while_forced_diagnostics_may_be_colored() {
    let output = fixture()
        .args(["--json", "--color=always", "diagnostic"])
        .output()
        .expect("fixture output");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"{\"ok\":true}\n");
    assert!(!output.stdout.contains(&0x1b));
    assert!(output.stderr.contains(&0x1b));
    assert!(String::from_utf8_lossy(&output.stderr).contains("diagnostic"));
}

#[test]
fn conflicting_shared_flags_fail_before_domain_execution() {
    let output = fixture()
        .args(["--json", "--no-json", "result"])
        .output()
        .expect("fixture output");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflict"));
}

#[test]
fn early_stdout_consumer_close_is_clean_process_termination() {
    let mut child = fixture()
        .args(["--json", "stream", "1000000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fixture");

    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);
    let mut first_record = String::new();
    reader.read_line(&mut first_record).expect("first record");
    assert_eq!(first_record, "{\"seq\":0}\n");
    drop(reader);

    let status = child.wait().expect("wait fixture");
    assert!(
        status.success(),
        "BrokenPipe must be treated as consumer close"
    );
}

#[test]
fn disabled_signal_policy_does_not_install_or_block() {
    let output = fixture()
        .arg("signal-lifecycle")
        .env("ORES_CLIS_SIGNAL_HANDLERS", "off")
        .output()
        .expect("disabled signal fixture");
    assert!(output.status.success());
    let record: Value = serde_json::from_slice(&output.stdout).expect("disabled JSON record");
    assert_eq!(record["mode"], "lifecycle");
    assert_eq!(record["status"], "disabled");
    assert!(output.stderr.is_empty());
}

#[test]
fn repeated_signal_installation_is_idempotent_in_one_process() {
    let output = fixture()
        .arg("signal-install-twice")
        .output()
        .expect("double install fixture");
    assert!(output.status.success());
    let record: Value = serde_json::from_slice(&output.stdout).expect("install JSON record");
    assert_eq!(record["first"], "installed");
    assert_eq!(record["second"], "already-installed");
}

#[test]
fn disabled_install_does_not_consume_the_global_installation_slot() {
    let output = fixture()
        .arg("signal-disabled-then-enabled")
        .output()
        .expect("disabled then enabled fixture");
    assert!(output.status.success());
    let record: Value = serde_json::from_slice(&output.stdout).expect("install JSON record");
    assert_eq!(record["first"], "disabled");
    assert_eq!(record["second"], "installed");
}

#[cfg(unix)]
mod unix_signals {
    use super::*;
    use std::process::{Child, ChildStderr, ChildStdout, ExitStatus};
    use std::thread;
    use std::time::Duration;

    struct RunningFixture {
        child: Child,
        stdout: BufReader<ChildStdout>,
        stderr: ChildStderr,
    }

    fn read_json_line(reader: &mut BufReader<ChildStdout>) -> Value {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read JSON line");
        assert!(
            !line.is_empty(),
            "fixture closed stdout before emitting evidence"
        );
        assert!(
            !line.as_bytes().contains(&0x1b),
            "machine evidence must remain ANSI-free"
        );
        serde_json::from_str(line.trim_end()).expect("valid fixture JSON")
    }

    fn spawn_signal_fixture(mode: &str) -> (RunningFixture, Value) {
        let mut child = fixture()
            .arg(mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn signal fixture");
        let stdout = child.stdout.take().expect("signal stdout");
        let stderr = child.stderr.take().expect("signal stderr");
        let mut running = RunningFixture {
            child,
            stdout: BufReader::new(stdout),
            stderr,
        };
        let ready = read_json_line(&mut running.stdout);
        (running, ready)
    }

    fn send_signal(child: &Child, signal: &str) {
        let status = Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(child.id().to_string())
            .status()
            .expect("invoke kill");
        assert!(status.success(), "kill -{signal} must succeed");
    }

    fn kill_and_collect(mut running: RunningFixture) -> (String, String) {
        running.child.kill().expect("force-stop signal fixture");
        running.child.wait().expect("wait killed signal fixture");
        let mut stdout = String::new();
        running
            .stdout
            .read_to_string(&mut stdout)
            .expect("read remaining stdout");
        let mut stderr = String::new();
        running
            .stderr
            .read_to_string(&mut stderr)
            .expect("read signal stderr");
        (stdout, stderr)
    }

    fn wait_and_collect(mut running: RunningFixture) -> (ExitStatus, String, String) {
        let status = running.child.wait().expect("wait signal fixture");
        let mut stdout = String::new();
        running
            .stdout
            .read_to_string(&mut stdout)
            .expect("read remaining stdout");
        let mut stderr = String::new();
        running
            .stderr
            .read_to_string(&mut stderr)
            .expect("read signal stderr");
        (status, stdout, stderr)
    }

    fn assert_lifecycle_ready(ready: &Value) {
        assert_eq!(ready["mode"], "lifecycle");
        assert_eq!(ready["status"], "installed");
    }

    #[test]
    fn noninteractive_sigint_emits_one_sigint_drain_without_ctrl_d_guidance() {
        let (mut running, ready) = spawn_signal_fixture("signal-lifecycle");
        assert_lifecycle_ready(&ready);
        send_signal(&running.child, "INT");
        let event = read_json_line(&mut running.stdout);
        assert_eq!(event["event"], "drain");
        assert_eq!(event["reason"], "sigint");
        let (remaining, stderr) = kill_and_collect(running);
        assert!(
            remaining.is_empty(),
            "one SIGINT must emit one lifecycle event"
        );
        assert!(
            !stderr.contains("Ctrl-D"),
            "redirected stdin must never advertise Ctrl-D"
        );
    }

    #[test]
    fn sigterm_emits_one_sigterm_drain() {
        let (mut running, ready) = spawn_signal_fixture("signal-lifecycle");
        assert_lifecycle_ready(&ready);
        send_signal(&running.child, "TERM");
        let event = read_json_line(&mut running.stdout);
        assert_eq!(event["event"], "drain");
        assert_eq!(event["reason"], "sigterm");
        let (remaining, _) = kill_and_collect(running);
        assert!(remaining.is_empty());
    }

    #[test]
    fn repeated_noninteractive_sigint_does_not_duplicate_or_force() {
        let (mut running, ready) = spawn_signal_fixture("signal-lifecycle");
        assert_lifecycle_ready(&ready);
        send_signal(&running.child, "INT");
        let first = read_json_line(&mut running.stdout);
        assert_eq!(first["event"], "drain");
        assert_eq!(first["reason"], "sigint");
        send_signal(&running.child, "INT");
        thread::sleep(Duration::from_millis(150));
        let (remaining, _) = kill_and_collect(running);
        assert!(
            remaining.is_empty(),
            "repeated SIGINT must be lifecycle-idempotent"
        );
    }

    #[test]
    fn repeated_sigterm_does_not_duplicate_or_force() {
        let (mut running, ready) = spawn_signal_fixture("signal-lifecycle");
        assert_lifecycle_ready(&ready);
        send_signal(&running.child, "TERM");
        let first = read_json_line(&mut running.stdout);
        assert_eq!(first["event"], "drain");
        assert_eq!(first["reason"], "sigterm");
        send_signal(&running.child, "TERM");
        thread::sleep(Duration::from_millis(150));
        let (remaining, _) = kill_and_collect(running);
        assert!(
            remaining.is_empty(),
            "repeated SIGTERM must be lifecycle-idempotent"
        );
    }

    #[test]
    fn sigint_then_sigterm_preserves_the_first_drain_reason() {
        let (mut running, ready) = spawn_signal_fixture("signal-lifecycle");
        assert_lifecycle_ready(&ready);
        send_signal(&running.child, "INT");
        let first = read_json_line(&mut running.stdout);
        assert_eq!(first["reason"], "sigint");
        send_signal(&running.child, "TERM");
        thread::sleep(Duration::from_millis(150));
        let (remaining, _) = kill_and_collect(running);
        assert!(
            remaining.is_empty(),
            "later SIGTERM must not replace the winning drain"
        );
    }

    #[test]
    fn sigterm_then_sigint_preserves_the_first_drain_reason() {
        let (mut running, ready) = spawn_signal_fixture("signal-lifecycle");
        assert_lifecycle_ready(&ready);
        send_signal(&running.child, "TERM");
        let first = read_json_line(&mut running.stdout);
        assert_eq!(first["reason"], "sigterm");
        send_signal(&running.child, "INT");
        thread::sleep(Duration::from_millis(150));
        let (remaining, stderr) = kill_and_collect(running);
        assert!(
            remaining.is_empty(),
            "later SIGINT must not replace the winning drain"
        );
        assert!(!stderr.contains("Ctrl-D"));
    }

    #[test]
    fn legacy_noninteractive_sigint_exits_with_130() {
        let (running, ready) = spawn_signal_fixture("signal-legacy");
        assert_eq!(ready["mode"], "legacy");
        assert_eq!(ready["status"], "installed");
        send_signal(&running.child, "INT");
        let (status, remaining, stderr) = wait_and_collect(running);
        assert_eq!(status.code(), Some(130));
        assert!(remaining.is_empty());
        assert!(stderr.contains("SIGINT received; shutting down process."));
        assert!(!stderr.contains("Ctrl-D"));
    }

    #[test]
    fn legacy_sigterm_exits_with_143() {
        let (running, ready) = spawn_signal_fixture("signal-legacy");
        assert_eq!(ready["mode"], "legacy");
        assert_eq!(ready["status"], "installed");
        send_signal(&running.child, "TERM");
        let (status, remaining, stderr) = wait_and_collect(running);
        assert_eq!(status.code(), Some(143));
        assert!(remaining.is_empty());
        assert!(stderr.contains("SIGTERM received; shutting down process."));
    }
}
