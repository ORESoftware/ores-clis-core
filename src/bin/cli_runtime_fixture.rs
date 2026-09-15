//! Process-boundary fixture used by `ores-clis-core` integration tests and downstream consumers.

use ores_clis_core::{
    CliPolicy, ColorRole, EmitDisposition, EnvironmentHints, ProtocolEmitter, StreamRole,
    TerminalState, paint, parse_shared_argv, top_level_io,
};
use std::io;

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
    let stdout = io::stdout();
    let mut emitter = ProtocolEmitter::new(stdout.lock(), StreamRole::Primary);
    if json {
        top_level_io(emitter.emit_primary_machine_record("{\"ok\":true}"))
    } else {
        let line = paint(color_stdout, ColorRole::Info, "ok");
        top_level_io(emitter.emit_primary_human_line(&line))
    }
}

fn emit_diagnostic_and_result(
    json: bool,
    color_stdout: bool,
    color_stderr: bool,
) -> io::Result<EmitDisposition> {
    let stderr = io::stderr();
    let mut diagnostics = ProtocolEmitter::new(stderr.lock(), StreamRole::Diagnostics);
    let line = paint(color_stderr, ColorRole::Warn, "diagnostic");
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
            let line = paint(color_stdout, ColorRole::Info, &format!("seq={sequence}"));
            emitter.emit_primary_human_line(&line)
        };
        match top_level_io(result)? {
            EmitDisposition::Written => {}
            EmitDisposition::ConsumerClosed => return Ok(EmitDisposition::ConsumerClosed),
        }
    }
    Ok(EmitDisposition::Written)
}
