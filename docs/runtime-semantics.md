# Shared CLI runtime semantics

This document defines the Rust-first behavior expected from `ores-clis-core` consumers. The shared crate owns runtime policy only; `flags-2-env` remains responsible for `.cli-flags.toml`, and `ores-otel` / `next-loggers` remain logging and telemetry implementations.

## Canonical shared flags

The parser-agnostic adapter consumes only the shared policy surface and preserves all other tokens for the consumer parser:

- `--color` means explicit color on human output.
- `--color=auto|always|never` chooses a canonical color mode.
- `--no-color` and `--!color` are explicit color-off forms.
- `--json` chooses structured primary output.
- `--no-json` and `--!json` choose human primary output.
- `--output=auto|human|json` or `--output VALUE` provide the explicit tri-state output form.
- `--log-level=silent|quiet|error|warn|info|debug|trace` or `--log-level LEVEL` choose the shared log threshold.
- `--quiet` and `--silent` remain compatibility aliases.
- `--` terminates shared parsing; remaining tokens are preserved verbatim.

Explicit duplicate values are idempotent. Contradictory explicit values are errors, never order-dependent last-write-wins behavior. Environment hints participate only during runtime resolution; an explicit CLI choice remains stronger than automatic environment-based behavior.

## Stream ownership

Default stream ownership is strict:

- stdout is the primary result and machine-protocol stream;
- stderr is the diagnostics, logging, and progress stream;
- machine records never contain ANSI escape bytes;
- one machine record never contains literal CR/LF framing bytes;
- progress and diagnostics must not be emitted through a primary machine-data emitter.

A command with a genuinely different wire protocol may opt out, but the deviation should be explicit in that command's docs and covered by compatibility tests rather than being an accidental local convention.

## Broken pipes and flushing

`BrokenPipe` represents normal downstream early termination at the CLI process boundary, such as piping a long result into `head`. `top_level_io` converts only `BrokenPipe` into `EmitDisposition::ConsumerClosed`; all unrelated I/O failures remain errors.

Per-record flushing is the default for terminals, pipes, and long-running streams. Bounded file-oriented commands may choose `FlushPolicy::OnDemand` and must flush explicitly before successful termination. Partial writes are handled through `Write::write_all`, and explicit flush failures are propagated.

## Adoption checklist

A Rust CLI adopting this crate should:

1. map existing parser output into the shared policy rather than replacing the parser wholesale;
2. preserve established output compatibility unless a separately reviewed breaking change is intended;
3. keep structured stdout free of logs, progress, and ANSI escapes;
4. route diagnostics and progress to stderr;
5. add `trace` where the CLI exposes shared log levels;
6. decide intentionally whether any legacy `silent` mode suppresses primary results or diagnostics only;
7. keep `NO_COLOR`, `FORCE_COLOR`, and `CLICOLOR_FORCE` behavior in the shared resolver;
8. treat a non-TTY stdout as JSON only where the command's compatibility contract allows it;
9. classify top-level broken pipes without swallowing other I/O errors;
10. keep `ores-otel` as the telemetry implementation rather than introducing a dependency cycle through this crate.

The authored TypeSpec and Draft 2020-12 JSON Schema remain independent peer authorities for wire shapes. TJSV is parity evidence between them and generated artifacts, not a replacement authority.
