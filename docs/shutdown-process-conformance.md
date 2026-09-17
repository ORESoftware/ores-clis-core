# Shutdown process conformance

The unit-level shutdown tests prove state transitions directly. `tests/process_harness.rs` now adds a second evidence layer: the compiled `cli_runtime_fixture` is started as a real child process with redirected stdio and receives operating-system signals through the host `kill` command on Unix.

This deliberately exercises the same boundary downstream CLIs and servers use after linking `ores-clis-core`; it does not replace the unit tests, the `ShutdownLifecycleGate` concurrency tests, or downstream server/middleware drain tests.

## Executable matrix

The process suite covers these invariants:

1. a lifecycle child publishes an explicit installed handshake before signal delivery;
2. machine evidence is newline-delimited JSON and ANSI-free;
3. redirected stdin makes SIGINT non-interactive, so Ctrl-D guidance is never advertised;
4. non-interactive SIGINT emits exactly one `drain/sigint` lifecycle event;
5. SIGTERM emits exactly one `drain/sigterm` lifecycle event;
6. repeated SIGINT does not emit a duplicate drain;
7. repeated SIGINT does not synthesize force escalation;
8. repeated SIGTERM does not emit a duplicate drain;
9. repeated SIGTERM does not synthesize force escalation;
10. SIGINT followed by SIGTERM preserves the first accepted drain reason;
11. SIGTERM followed by SIGINT preserves the first accepted drain reason;
12. legacy non-interactive SIGINT exits with code 130;
13. legacy SIGTERM exits with code 143;
14. `ORES_CLIS_SIGNAL_HANDLERS=off` returns `disabled` without installing or blocking;
15. a second enabled installation in the same process returns `already-installed`;
16. a disabled setup call does not consume the one enabled installation slot.

The duplicate and mixed-signal checks intentionally wait only after the first event has been observed. This binds the test to a known-installed handler and a known-completed first transition before probing idempotency, reducing scheduler-race ambiguity.

## Platform boundary

Real SIGINT/SIGTERM delivery tests are `#[cfg(unix)]` because Unix exposes both signals and a standard `kill` utility. The fixture modes themselves compile on Windows so the cross-platform crate surface remains covered by the normal build/test matrix; Windows Ctrl-C behavior continues to use the `ctrlc` backend and unit-level transition tests.

## Ownership boundary

These tests prove process-event capture and lifecycle ordering only. They do not claim that an HTTP server stopped accepting connections, that requests drained, that telemetry flushed, or that a transport honored a shutdown deadline. Those remain consumer and `ores-middleware` responsibilities, with `ShutdownLifecycleGate` available only as a process-intent deduplication primitive.
