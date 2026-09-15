# CLI runtime contract v1

`main.tsp` and `authored.schema.json` are independent peer authorities. Neither is generated from the other. `tjsv check` must admit their semantic parity before release; any TypeSpec-generated JSON Schema is comparison evidence only.

The runtime invariants are:

- pre-resolution `CliPolicy.output_mode=auto` resolves from **stdout**: terminal => human, non-terminal => JSON/NDJSON.
- resolved output is intentionally narrower: `ResolvedCliPolicy.output_mode` may be only `human` or `json`; a resolved `auto` value is invalid contract state.
- the instance corpus proves valid TTY-human and piped-JSON states and rejects unresolved-auto and unknown-log-level controls.
- `color_mode=auto` resolves per destination stream and defaults off for a non-TTY stream.
- `--color` means `always`; `--no-color` / `--!color` mean `never`.
- JSON/NDJSON **stdout** is always ANSI-free. Stderr diagnostics are a separate human channel and may remain colored when stderr is a TTY, including while stdout is piped as JSON.
- `NO_COLOR` disables automatic color; `CLICOLOR_FORCE` and `FORCE_COLOR` enable automatic color when truthy. Explicit CLI color policy wins over those environment hints.
- log levels are `silent`, `quiet`, `error`, `warn`, `info`, `debug`, and `trace`.
- log filtering never suppresses the primary command result. Logs/progress and result output are separate channels.
- primary command results and machine protocols belong on stdout; diagnostics/progress belong on stderr.
- one-record machine framing is ANSI-free and contains no literal CR/LF framing bytes.
- streaming records are newline-delimited and flushed after each record by default, with an explicit on-demand option for bounded output.
- a downstream `BrokenPipe` such as `tool | head` is normal consumer termination; unrelated I/O failures remain errors.
- machine-readable telemetry transports must remain ANSI-free independently of terminal color policy.

`ores-clis-core` owns CLI runtime policy, not telemetry. `ores-otel` remains the logging/telemetry implementation authority, and `ORESoftware/ores-interfaces` remains the cross-repository shared-interface authority.

Rust is the first implementation. Future language implementations must consume this contract rather than defining a parallel policy vocabulary.
