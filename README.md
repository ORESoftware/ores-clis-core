# ores-clis-core

Shared Rust CLI runtime policy for ORESoftware CLIs.

The crate centralizes terminal detection, human-vs-JSON output selection, color policy, log-level filtering, streaming/flush behavior, parser-agnostic shared flag handling, and primary-vs-diagnostic stream ownership. Contract authorities live under `contracts/cli-runtime/v1` and are checked with `typespec-json-schema-validator`; cross-repository interface authority remains `ORESoftware/ores-interfaces`.

Initial rollout target: Rust CLIs only. Other language adapters should consume the same contract later rather than re-inventing policy.

## Shared flag surface

Consumers may use `parse_shared_argv` to extract the common policy layer before handing the remaining argv to their own parser. The shared layer supports `--color`, `--color=auto|always|never`, `--no-color`, `--!color`, `--json`, `--no-json`, `--!json`, `--output=auto|human|json`, and `--log-level=silent|quiet|error|warn|info|debug|trace`, plus the compatibility aliases `--quiet` and `--silent`.

Conflicting explicit choices fail deterministically. Duplicate equivalent choices are idempotent. Unrelated argv is preserved for the consumer parser, and `--` terminates shared parsing.

## Output and stream boundary

Primary command results and machine protocols belong on stdout. Diagnostics, progress, and operator-facing logging belong on stderr. JSON/NDJSON records are ANSI-free and single-line. A normal downstream `BrokenPipe` such as `tool | head` is classified as clean consumer termination; unrelated I/O failures remain errors.

Color is resolved independently per stream. Automatic color follows the destination TTY and conventional environment hints. Structured primary stdout never receives ANSI escapes, while an attached stderr may remain colored even when stdout is piped as JSON.

`ores-clis-core` does not implement telemetry. `ores-otel` remains the logging/telemetry implementation authority, and consumers bridge the resolved shared log threshold into their logger without allowing log filtering to accidentally own primary command-result semantics.

## Adoption policy

Existing CLIs should preserve established protocol compatibility during migration. A command that intentionally differs from the automatic human/JSON policy may keep an explicit local default, but the deviation should be documented and tested rather than silently re-implementing TTY, color, log-level, or stream-routing semantics.
