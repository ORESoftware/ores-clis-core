# Conformance

`contracts/` is the authority for shared CLI/runtime API and data-shape semantics in `ORESoftware/ores-clis-core`. `conformance/` is the shared behavioral corpus that CLI libraries, adapters, command runtimes, and compatibility layers must consume.

## Rules

1. Keep shared behavioral vectors under `conformance/cases/` and feed the same case bytes to every implementation under test.
2. Do not maintain implementation-specific golden vectors. A case may describe implementation-neutral inputs and a normalized expected receipt, but the expected result must be shared.
3. Before promotion, bind evidence to the exact current contract inputs and conformance-case digests. Stale evidence fails closed.
4. Missing evidence from any implementation or adapter declared required by the promotion gate is a failure, not a skip.
5. Generated reports, normalized receipts, parity reports, and other artifacts are evidence only. They do not become contract or conformance authority.
6. `contracts/` remains authoritative for structure and wire shape; `conformance/` owns shared behavioral expectations. Neither directory silently rewrites the other.

The initial `cases/bootstrap.v1.json` document establishes this repository-level authority boundary only. It does **not** prove command/runtime behavioral equivalence. Add domain-specific cases as normalized inputs plus normalized expected receipts, then make every relevant CLI/runtime implementation execute the same corpus.

Where a contract already carries a shared instance corpus, conformance runners should reuse or reference those exact bytes instead of creating divergent command-local copies.
