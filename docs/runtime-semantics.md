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

## Environment inventory and defaults

`.zpkg.toml` carries a `[[env]]` inventory for every process environment variable read by this library. These entries reuse the ORES env-manifest vocabulary (`name`, `key`, `kind`, `required`, `secret`, `exposure`, `description`, `overrides`, `environments`, and optional `defaultValue`). The declaration is package/input metadata, not a second runtime configuration authority and not a plaintext secret store.

The complete current inventory is:

| Key | Library-owned fallback | Semantic target |
| --- | --- | --- |
| `NO_COLOR` | absent / no override | `environment_hints.no_color` |
| `CLICOLOR` | absent / no override | `environment_hints.no_color` |
| `CLICOLOR_FORCE` | absent / no override | `environment_hints.force_color` |
| `FORCE_COLOR` | absent / no override | `environment_hints.force_color` |
| `TERM` | absent / no override | `environment_hints.no_color` when equal to `dumb` |
| `ORES_CLIS_SIGNAL_HANDLERS` | `true` | `signal_handlers.enabled` |
| `ORES_CLIS_SIGNAL_TTY_REQUIREMENT` | `stdin` | `signal_handlers.tty_requirement` |

The first five keys are established shell/terminal conventions. `ores-clis-core` reads them but does not own their ambient values, so `.zpkg.toml` must not fabricate defaults for them. This is particularly important for `NO_COLOR`: the code is presence-based, so setting an empty default would change behavior rather than describe it.

The two `ORES_CLIS_*` keys are owned by this library, and their `defaultValue` entries mirror the code defaults. All seven entries are non-secret and `env-only`; secrets must stay in the secret-store/environment boundary rather than being given defaults in package metadata.

## Optional signal and interactive shutdown policy

Signal interception is never implicit. Importing the crate leaves the operating system's normal signal behavior untouched; a CLI must explicitly call `setup_signal_handlers()` or `setup_signal_handlers_with(...)`.

The default installed policy is:

- SIGINT + TTY stdin: emit a diagnostic on stderr telling the operator to use Ctrl-D, arm a stdin EOF/Ctrl-D waiter, and keep running.
- SIGINT + non-TTY stdin: emit a diagnostic and terminate with the conventional code 130.
- SIGTERM on Unix: emit a diagnostic and terminate immediately with the conventional code 143, regardless of TTY state.
- Ctrl-D/EOF after the interactive SIGINT path: emit a diagnostic and terminate cleanly with code 0.
- Repeated setup calls are idempotent within the process.

stdin is the mandatory interactive signal because Ctrl-D is an input/EOF gesture. stdout and stderr TTY state may further restrict interactive handling, but they never substitute for non-TTY stdin. The supported stricter requirements are `stdin+stdout`, `stdin+stderr`, and `all`.

`ORES_CLIS_SIGNAL_HANDLERS=0|false|no|off` disables an explicit setup call, while the true spellings `1|true|yes|on` enable it. `ORES_CLIS_SIGNAL_TTY_REQUIREMENT=stdin|stdin+stdout|stdin+stderr|all` controls the TTY requirement; `stdin+stdout+stderr` is also accepted as an alias for `all`. Missing environment values preserve the default: setup enabled after an explicit function call, with stdin as the only required TTY.

Consumers with cleanup work should use `setup_signal_handlers_with(...)`. Its callback is delivered at most once with a `ShutdownReason` and owns the final shutdown action; this is where consumers can flush `ores-otel`, cancel runtimes, drain workers, or otherwise perform graceful shutdown before exiting.

## Adoption checklist

A Rust CLI adopting this crate should:

1. map existing parser output into the shared policy rather than replacing the parser wholesale;
2. preserve established output compatibility unless a separately reviewed breaking change is intended;
3. keep structured stdout free of logs, progress, and ANSI escapes;
4. route diagnostics and progress to stderr;
5. add `trace` where the CLI exposes shared log levels;
6. decide intentionally whether any legacy `silent` mode suppresses primary results or diagnostics only;
7. keep `NO_COLOR`, `CLICOLOR`, `FORCE_COLOR`, `CLICOLOR_FORCE`, and `TERM` behavior in the shared resolver;
8. treat a non-TTY stdout as JSON only where the command's compatibility contract allows it;
9. classify top-level broken pipes without swallowing other I/O errors;
10. keep the `.zpkg.toml` env inventory synchronized with every ambient variable read by this crate and never put secret values there;
11. call the shared signal setup only in CLIs that intentionally opt into Ctrl-D-confirmed interactive shutdown;
12. keep `ores-otel` as the telemetry implementation rather than introducing a dependency cycle through this crate.

The authored TypeSpec and Draft 2020-12 JSON Schema remain independent peer authorities for wire shapes. TJSV is parity evidence between them and generated artifacts, not a replacement authority.
