//! Process-boundary fixture used by `ores-clis-core` integration tests and downstream consumers.

use ores_clis_core::{
    ColorRole, EmitDisposition, EnvironmentHints, ProtocolEmitter, ShutdownAction, ShutdownReason,
    SignalHandlerOptions, SignalHandlerStatus, StreamRole, TerminalState, paint, parse_shared_argv,
    setup_signal_handlers, setup_signal_handlers_with, setup_signal_handlers_with_lifecycle,
    top_level_io,
};
use serde_json::json;
use std::io::{self, Write};
use std::thread;

fn main() {
    let shared = match parse_shared_argv(std::env::args().skip(1)) {
        Ok(shared) => shared,
        Err(error) => {
            eprintln!("fixture: {error}");
            std::process::exit(2);
        }
    };
    let runtime = shared
        .policy
        .resolve(TerminalState::detect(), EnvironmentHints::detect());

    let command = shared
        .passthrough
        .first()
        .map(String::as_str)
        .unwrap_or("result");
    let result = match command {
        "result" => emit_result(runtime.json(), runtime.color_stdout()),
        "diagnostic" => emit_diagnostic_and_result(
            runtime.json(),
            runtime.color_stdout(),
            runtime.color_stderr(),
        ),
        "stream" => {
            let count = shared
                .passthrough
                .get(1)
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(3);
            emit_stream(runtime.json(), runtime.color_stdout(), count)
        }
        "signal-lifecycle" => run_lifecycle_signal_fixture(),
        "signal-legacy" => run_legacy_signal_fixture(),
        "signal-install-twice" => run_double_install_fixture(),
        "signal-disabled-then-enabled" => run_disabled_then_enabled_fixture(),
        unknown => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown fixture command: {unknown}"),
        )),
    };

    match result {
        Ok(EmitDisposition::Written | EmitDisposition::ConsumerClosed) => {}
        Err(error) => {
            eprintln!("fixture I/O error: {error}");
            std::process::exit(1);
        }
    }
}

fn emit_result(json: bool, color_stdout: bool) -> io::Result<EmitDisposition> {
    if json {
        emit_machine_record("{\"ok\":true}")
    } else {
        let stdout = io::stdout();
        let mut emitter = ProtocolEmitter::new(stdout.lock(), StreamRole::Primary);
        let line = paint(color_stdout, ColorRole::Info, "ok");
        top_level_io(emitter.emit_primary_human_line(&line))
    }
}

fn emit_machine_record(record: &str) -> io::Result<EmitDisposition> {
    let disposition = {
        let stdout = io::stdout();
        let mut emitter = ProtocolEmitter::new(stdout.lock(), StreamRole::Primary);
        top_level_io(emitter.emit_primary_machine_record(record))?
    };
    io::stdout().flush()?;
    Ok(disposition)
}

fn emit_diagnostic_and_result(
    json: bool,
    color_stdout: bool,
    color_stderr: bool,
) -> io::Result<EmitDisposition> {
    let stderr = io::stderr();
    let mut diagnostics = ProtocolEmitter::new(stderr.lock(), StreamRole::Diagnostics);
    let line = paint(color_stderr, ColorRole::Warning, "diagnostic");
    match top_level_io(diagnostics.emit_diagnostic_line(&line))? {
        EmitDisposition::Written => emit_result(json, color_stdout),
        EmitDisposition::ConsumerClosed => Ok(EmitDisposition::ConsumerClosed),
    }
}

fn emit_stream(json: bool, color_stdout: bool, count: usize) -> io::Result<EmitDisposition> {
    let stdout = io::stdout();
    let mut emitter = ProtocolEmitter::new(stdout.lock(), StreamRole::Primary);
    for sequence in 0..count {
        let result = if json {
            emitter.emit_primary_machine_record(&format!("{{\"seq\":{sequence}}}"))
        } else {
            let line = paint(color_stdout, ColorRole::Info, format!("seq={sequence}"));
            emitter.emit_primary_human_line(&line)
        };
        match top_level_io(result)? {
            EmitDisposition::Written => {}
            EmitDisposition::ConsumerClosed => return Ok(EmitDisposition::ConsumerClosed),
        }
    }
    Ok(EmitDisposition::Written)
}

fn signal_error(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(error)
}

fn status_name(status: SignalHandlerStatus) -> &'static str {
    match status {
        SignalHandlerStatus::Installed => "installed",
        SignalHandlerStatus::Disabled => "disabled",
        SignalHandlerStatus::AlreadyInstalled => "already-installed",
    }
}

fn reason_name(reason: ShutdownReason) -> &'static str {
    match reason {
        ShutdownReason::SigInt => "sigint",
        ShutdownReason::SigTerm => "sigterm",
        ShutdownReason::CtrlD => "ctrl-d",
    }
}

fn emit_lifecycle_action(action: ShutdownAction) {
    let event = if action.is_force() { "force" } else { "drain" };
    let record = json!({
        "event": event,
        "reason": reason_name(action.reason()),
    });
    if emit_machine_record(&record.to_string()).is_err() {
        std::process::exit(1);
    }
    if action.is_force() {
        std::process::exit(0);
    }
}

fn run_lifecycle_signal_fixture() -> io::Result<EmitDisposition> {
    let options = SignalHandlerOptions::from_env().map_err(signal_error)?;
    let status =
        setup_signal_handlers_with_lifecycle(options, emit_lifecycle_action).map_err(signal_error)?;
    let disposition = emit_machine_record(
        &json!({"mode": "lifecycle", "status": status_name(status)}).to_string(),
    )?;
    if status != SignalHandlerStatus::Installed {
        return Ok(disposition);
    }
    loop {
        thread::park();
    }
}

fn run_legacy_signal_fixture() -> io::Result<EmitDisposition> {
    let status = setup_signal_handlers().map_err(signal_error)?;
    let disposition = emit_machine_record(
        &json!({"mode": "legacy", "status": status_name(status)}).to_string(),
    )?;
    if status != SignalHandlerStatus::Installed {
        return Ok(disposition);
    }
    loop {
        thread::park();
    }
}

fn run_double_install_fixture() -> io::Result<EmitDisposition> {
    let options = SignalHandlerOptions::new(TerminalState::new(false, false, false));
    let first = setup_signal_handlers_with(options, |_| {}).map_err(signal_error)?;
    let second = setup_signal_handlers_with(options, |_| {}).map_err(signal_error)?;
    emit_machine_record(
        &json!({
            "first": status_name(first),
            "second": status_name(second),
        })
        .to_string(),
    )
}

fn run_disabled_then_enabled_fixture() -> io::Result<EmitDisposition> {
    let options = SignalHandlerOptions::new(TerminalState::new(false, false, false));
    let first = setup_signal_handlers_with(options.with_enabled(false), |_| {}).map_err(signal_error)?;
    let second = setup_signal_handlers_with(options.with_enabled(true), |_| {}).map_err(signal_error)?;
    emit_machine_record(
        &json!({
            "first": status_name(first),
            "second": status_name(second),
        })
        .to_string(),
    )
}
