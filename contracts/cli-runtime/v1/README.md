# CLI runtime contract v1

`main.tsp` and `authored.schema.json` are independent peer authorities. Neither is generated from the other. `tjsv check` must admit their semantic parity before release.

The runtime invariants are:

- `output_mode=auto` resolves from **stdout**: terminal => human, non-terminal => JSON/NDJSON.
- `color_mode=auto` resolves per destination stream and defaults off for a non-TTY stream.
- `--color` means `always`; `--no-color` means `never`. JSON output is always ANSI-free.
- `NO_COLOR` disables automatic color; `CLICOLOR_FORCE` and `FORCE_COLOR` enable automatic color when truthy. Explicit CLI color policy wins over those environment hints.
- log levels are `silent`, `quiet`, `error`, `warn`, `info`, `debug`, and `trace`.
- log filtering never suppresses the primary command result. Logs/progress and result output are separate channels.
- streaming records are newline-delimited and flushed after each record by default.

Rust is the first implementation. Future language implementations must consume this contract rather than defining a parallel policy vocabulary.
