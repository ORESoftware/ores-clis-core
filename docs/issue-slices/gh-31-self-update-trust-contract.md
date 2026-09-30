# self-update trust boundary

Driver: `ORESoftware/ores-clis-core#31`

This document defines one bounded, independently reviewable contract slice. It advances the driver issue without claiming full implementation.

## Invariants

- Bound download size and redirect/origin behavior before reading update artifacts.
- Verify immutable release identity and cryptographic digest before extraction.
- Reject path traversal, symlink escapes, and unexpected archive entry types.
- Replace executables atomically and preserve a fail-safe rollback path.

## Verification

- Test/verify the exact PR head.
- Preserve fail-closed behavior for malformed or untrusted inputs.
- Treat skipped/zero-step CI as missing evidence.
- Bind release or runtime evidence to immutable source identity.

## Non-goals

This slice does not add secrets, weaken repository protection, or silently rewrite consumer state.
