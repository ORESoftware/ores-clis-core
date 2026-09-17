# ores-clis-core

Shared Rust CLI runtime policy for ORESoftware CLIs.

The crate centralizes terminal detection, human-vs-JSON output selection, color policy, log-level filtering, streaming/flush behavior, parser-agnostic shared flag handling, primary-vs-diagnostic stream ownership, and opt-in process signal behavior. Contract authorities live under `contracts/cli-runtime/v1` and are checked with `typespec-json-schema-validator`; cross-repository interface authority remains `ORESoftware/ores-interfaces`.

Initial rollout target: Rust CLIs only. Other language adapters should consume the same contract later rather than re-inventing policy.

## Shared flag surface

Consumers may use `parse_shared_argv` to extract the common policy layer before handing the remaining argv to their own parser. The shared layer supports `--color`, `--color=auto|always|never`, `--no-color`, `--!color`, `--json`, `--no-json`, `--!json`, `--output=auto|human|json`, and `--log-level=silent|quiet|error|warn|info|debug|trace`, plus the compatibility aliases `--quiet` and `--silent`.

Conflicting explicit choices fail deterministically. Duplicate equivalent choices are idempotent. Unrelated argv is preserved for the consumer parser, and `--` terminates shared parsing.

## Output and stream boundary

Primary command results and machine protocols belong on stdout. Diagnostics, progress, and operator-facing logging belong on stderr. JSON/NDJSON records are ANSI-free and single-line. A normal downstream `BrokenPipe` such as `tool | head` is classified as clean consumer termination; unrelated I/O failures remain errors.

Color is resolved independently per stream. Automatic color follows the destination TTY and conventional environment hints. Structured primary stdout never receives ANSI escapes, while an attached stderr may remain colored even when stdout is piped as JSON.

`ores-clis-core` does not implement telemetry. `ores-otel` remains the logging/telemetry implementation authority, and consumers bridge the resolved shared log threshold into their logger without allowing log filtering to accidentally own primary command-result semantics.

## Environment contract

The environment variables this SDK reads are documented here and in `docs/runtime-semantics.md`. `.zpkg.toml` intentionally does not duplicate this inventory: the current Zed package-manifest schema does not define arbitrary ambient process-variable declarations, and Zed's environment-plan model describes manager/tool/system-package provenance rather than runtime process-input enumeration.

| Variable | Default when absent | Effect |
| --- | --- | --- |
| `NO_COLOR` | no override | Presence disables automatic color; its value is ignored. |
| `CLICOLOR` | no override | `0`, `false`, `no`, or `off` disables automatic color. |
| `CLICOLOR_FORCE` | no override | Any non-empty value other than `0`, `false`, `no`, or `off` requests automatic color. |
| `FORCE_COLOR` | no override | Same force-color semantics as `CLICOLOR_FORCE`. |
| `TERM` | no override | `dumb` disables automatic color; other values do not change this library's color decision. |
| `ORES_CLIS_SIGNAL_HANDLERS` | `true` | Controls whether an explicit signal-handler setup call installs handlers. |
| `ORES_CLIS_SIGNAL_TTY_REQUIREMENT` | `stdin` | Chooses `stdin`, `stdin+stdout`, `stdin+stderr`, or `all` for interactive SIGINT eligibility; `stdin+stdout+stderr` is accepted as an alias for `all`. |

The conventional terminal/color variables intentionally have no fabricated defaults because absence is semantically meaningful, especially for presence-based `NO_COLOR`. The two `ORES_CLIS_*` values are library-owned fallbacks applied by the runtime after an explicit setup call. Secret values belong at the secret-store/process-environment boundary, not in package metadata or committed documentation.

## Optional signal and Ctrl-D shutdown policy

Signal handling is explicitly opt-in. Importing the crate does not replace the platform's normal SIGINT behavior; a consumer must call `setup_signal_handlers()` (or the callback variant) to install the shared policy.

```rust
use ores_clis_core::setup_signal_handlers;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _status = setup_signal_handlers()?;
    // Run the CLI.
    Ok(())
}
```

With the default setup:

- If stdin is a TTY, SIGINT logs `use Ctrl-D to shutdown process`, arms a Ctrl-D/EOF waiter, and keeps the process alive until Ctrl-D is received.
- If stdin is not a TTY, SIGINT logs a shutdown message and exits with code 130.
- SIGTERM always logs a shutdown message and exits with code 143 on Unix; it never waits for Ctrl-D.
- Ctrl-D/EOF after the interactive SIGINT path exits cleanly with code 0.
- The handler is process-global and repeated setup calls are idempotent.

stdin is intentionally authoritative because Ctrl-D is an input/EOF gesture. stdout and stderr can differ from stdin in pipelines and redirection; consumers that want a stricter interactive definition may require `stdin+stdout`, `stdin+stderr`, or all three streams through `SignalHandlerOptions` or `ORES_CLIS_SIGNAL_TTY_REQUIREMENT`.

Two environment switches are recognized when the setup function is called:

- `ORES_CLIS_SIGNAL_HANDLERS=1|true|yes|on` enables installation and `0|false|no|off` disables it. The default is enabled **only after the consumer calls the setup function**.
- `ORES_CLIS_SIGNAL_TTY_REQUIREMENT=stdin|stdin+stdout|stdin+stderr|all` tightens the TTY requirement for the interactive SIGINT path. `stdin+stdout+stderr` is accepted as an alias for `all`; stdin remains mandatory in every mode.

Consumers that need graceful cleanup should use `setup_signal_handlers_with(...)`. The callback is invoked at most once with `ShutdownReason` and owns the final shutdown action, allowing the CLI to flush logs, cancel work, drain resources, or coordinate an async runtime before exiting.

## Adoption policy

Existing CLIs should preserve established protocol compatibility during migration. A command that intentionally differs from the automatic human/JSON policy may keep an explicit local default, but the deviation should be documented and tested rather than silently re-implementing TTY, color, log-level, or stream-routing semantics.
