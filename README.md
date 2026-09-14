# ores-clis-core

Shared Rust CLI runtime policy for ORESoftware CLIs.

The crate centralizes terminal detection, human-vs-JSON output selection, color policy, log-level filtering, and streaming/flush behavior. Contract authorities live under `contracts/cli-runtime/v1` and are checked with `typespec-json-schema-validator`; cross-repository interface authority is `ORESoftware/ores-interfaces`.

Initial rollout target: Rust CLIs only. Other language adapters should consume the same contract later rather than re-inventing policy.
