//! Exercises the shared CLI runtime through real child-process stdout/stderr pipe boundaries.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn fixture() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cli_runtime_fixture"));
    for key in ["NO_COLOR", "CLICOLOR_FORCE", "FORCE_COLOR"] {
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
    assert!(status.success(), "BrokenPipe must be treated as consumer close");
}
