# Process shutdown lifecycle gate

`ShutdownLifecycleGate` is an optional process-intent deduplication primitive for long-running ORES CLIs, workers, and servers that combine more than one shutdown trigger.

It has exactly three monotonic phases:

- `running`
- `draining`
- `forced`

Only `running -> draining -> forced` is admitted. A force request while still running is rejected rather than skipping the grace phase. Duplicate drain/force requests and a drain request after force are ignored.

## Relationship to signal handling

`setup_signal_handlers_with_lifecycle(...)` remains the process-signal adapter. It emits `ShutdownAction::Drain(reason)` and `ShutdownAction::Force(reason)` with the original signal/terminal reason.

A consumer that also has another shutdown source can pass those actions through `ShutdownLifecycleGate::try_apply(...)` and use the boolean result as its exactly-once orchestration boundary. The gate does not replace the signal adapter and does not install signal handlers.

## Relationship to `ores-middleware`

The names intentionally match the conceptual lifecycle without creating a crate dependency or moving HTTP ownership here. A server consumer can use an accepted process transition as follows:

1. on accepted drain intent, stop listener/ingress acceptance;
2. project drain intent into `ores-middleware::ShutdownCoordinator`;
3. wait for application-request drain and transport graceful shutdown under the server's own deadline policy;
4. flush telemetry and close resources owned by the process;
5. on accepted force intent, project explicit escalation to the middleware/server transport and terminate according to consumer policy.

`ShutdownLifecycleGate::is_accepting_work()` is an intent observation only. It is not a request-admission middleware and must not be used as a substitute for the race-safe admission guard at the HTTP/application boundary.

## Multi-source example

A server may receive shutdown intent from SIGINT/SIGTERM, an administrative control plane, and an orchestrator hook. All three sources may race. The gate ensures only one source wins the transition into draining and only one post-drain source wins force escalation.

The source-specific reason should still be logged or recorded by the consumer. The gate intentionally stores only phase because it is responsible for ordering/deduplication, not audit provenance or exit-code policy.

## Failure model

The phase is stored as one atomic byte. Reads use acquire ordering and transitions use compare-and-exchange with acquire/release ordering. An impossible internal byte value is interpreted as `forced`, making the observation fail closed rather than reporting that new work is safe to admit.
