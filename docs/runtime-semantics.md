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

The ambient process variables consumed by this library are part of the SDK/runtime contract documented here. They are deliberately not encoded in `.zpkg.toml`: the current Zed package-manifest schema does not define arbitrary ambient environment-variable inventory entries, while Zed's environment-plan schema models development-environment manager/tool/system-package provenance rather than process-input declarations.

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

The first five keys are established shell/terminal conventions. `ores-clis-core` reads them but does not own their ambient values, so no committed package metadata may fabricate defaults for them. This is particularly important for `NO_COLOR`: the code is presence-based, so injecting even an empty default changes behavior rather than merely describing it.

The two `ORES_CLIS_*` keys are owned by this library and their documented fallbacks mirror the code defaults after a consumer explicitly calls signal setup. All seven are non-secret names; secret values must stay in the secret-store/process-environment boundary rather than package metadata or source control.

## Optional signal and interactive shutdown policy

Signal interception is never implicit. Importing the crate leaves the operating system's normal signal behavior untouched; a consumer must explicitly call one of the signal setup functions.

### Legacy one-shot policy

`setup_signal_handlers()` and `setup_signal_handlers_with(...)` preserve the established CLI behavior:

- SIGINT + TTY stdin: emit a diagnostic on stderr telling the operator to use Ctrl-D, arm one stdin EOF/Ctrl-D waiter, and keep running without invoking the callback yet.
- SIGINT + non-TTY stdin: emit a diagnostic and perform the one-shot shutdown callback; the default wrapper exits with conventional code 130.
- SIGTERM on Unix: emit a diagnostic and perform the callback immediately; the default wrapper exits with conventional code 143.
- Ctrl-D/EOF after the interactive SIGINT path: perform the one-shot callback; the default wrapper exits cleanly with code 0.
- The callback is delivered at most once and repeated setup calls are idempotent within the process.

This surface is intentionally retained for compatibility. Existing commands do not silently change from “Ctrl-D confirms shutdown” to “SIGINT starts cleanup.”

### Two-phase graceful lifecycle policy

Long-running servers and workers that need to start cleanup on first SIGINT use `setup_signal_handlers_with_lifecycle(...)`. The callback receives `ShutdownAction`, separating operator intent to begin graceful drain from intent to force an already-started drain.

The lifecycle invariants are:

1. First interactive SIGINT emits exactly one `ShutdownAction::Drain(ShutdownReason::SigInt)` immediately.
2. That first interactive SIGINT arms exactly one stdin Ctrl-D/EOF waiter and tells the operator Ctrl-D will force shutdown.
3. Repeated interactive SIGINT does not emit another drain and does not implicitly escalate to force.
4. Ctrl-D/EOF after a drain emits exactly one `ShutdownAction::Force(ShutdownReason::CtrlD)`.
5. A force event is never emitted before a drain event.
6. Non-interactive SIGINT emits `Drain(SigInt)` directly; no Ctrl-D waiter is required.
7. SIGTERM on Unix emits `Drain(SigTerm)` and never implicitly emits force.
8. Failure to spawn the Ctrl-D waiter after drain has started leaves the drain active; an implementation failure is not interpreted as operator force intent.
9. A later stdin read failure on the lifecycle waiter likewise leaves the drain active.
10. Lifecycle drain and force delivery are independently idempotent.

The process-event layer deliberately owns no HTTP behavior. A server may map `Drain` to stopping listener/ingress acceptance plus `ores-middleware::ShutdownCoordinator::start_draining()` / drain orchestration, and map `Force` to `force_shutdown()`. `ores-clis-core` does not depend on `ores-middleware`, select an HTTP rejection code, account HTTP handlers, close protocol streams, flush `ores-otel`, or terminate an async runtime. Those remain consumer/transport/middleware/telemetry responsibilities.

stdin is the mandatory interactive signal because Ctrl-D is an input/EOF gesture. stdout and stderr TTY state may further restrict interactive handling, but they never substitute for non-TTY stdin. The supported stricter requirements are `stdin+stdout`, `stdin+stderr`, and `all`.

`ORES_CLIS_SIGNAL_HANDLERS=0|false|no|off` disables an explicit setup call, while the true spellings `1|true|yes|on` enable it. `ORES_CLIS_SIGNAL_TTY_REQUIREMENT=stdin|stdin+stdout|stdin+stderr|all` controls the TTY requirement; `stdin+stdout+stderr` is also accepted as an alias for `all`. Missing environment values preserve the default: setup enabled after an explicit function call, with stdin as the only required TTY.

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
10. keep this documented ambient-variable inventory synchronized with every environment read by the crate and never commit secret values;
11. use the legacy one-shot signal API only where Ctrl-D-confirmed shutdown is still the intended contract;
12. use the lifecycle signal API for servers/workers where first SIGINT must begin drain and Ctrl-D must explicitly force;
13. never reinterpret repeated SIGINT, waiter-spawn failure, or waiter-read failure as force intent;
14. keep listener/protocol drain and HTTP admission in the server/middleware layer rather than this crate;
15. keep `ores-otel` as the telemetry implementation rather than introducing a dependency cycle through this crate.

The authored TypeSpec and Draft 2020-12 JSON Schema remain independent peer authorities for wire shapes. TJSV is parity evidence between them and generated artifacts, not a replacement authority.
