# ADR-001 — Lance as the on-disk storage format

- **Date:** 2026-08-21
- **Status:** Accepted

## Context

EuleDB needs a single-file, columnar, embedded on-disk format that supports zero-copy reads, random
access, versioning and native vector indices, on commodity hardware, from a workstation down to a low-power
single-board computer (spec § Platform classes).

Writing an own format is the obvious temptation for a database project, and the research estimate puts
that work at 3-5 person-months of the 17-21 total. That is a quarter of the budget spent on the layer
where EuleDB has no differentiator: the competitive gap against LanceDB, Qdrant and a DuckDB+LLM
wrapper is the validated NL/IR sandbox, the fusion planner, CRDT sync and the child-accessible UX —
not the byte layout on disk.

Lance also already ships hybrid search (vector + BM25 + SQL over one dataset) and IVF-PQ, which raises
the sharper question: how much is left to build at all? Enough — none of it in storage.

## Decision

Use **Lance** as the on-disk format, pinned to an exact version, reached only through an internal
storage trait so that no module outside the storage crate references a Lance type (AC-14).

Apache Arrow (`arrow-rs`) stays the in-memory representation and the interchange contract.

## Consequences

**Positive.**
- 3-5 person-months redirected from storage to the differentiating layers.
- Random access, versioning, zstd/FSST-class string encoding and IVF-PQ arrive for free.
- Arrow-native, so the eventual zero-copy Python path (AC-56) needs no conversion layer.

**Negative, and accepted.**
- Lance is under active development; format and API changes are possible. Mitigated by the exact
  version pin and the trait boundary — an upgrade is one crate's problem, not the codebase's.
- The published "100x / 2000x faster than Parquet" figures are the Lance project's own random-access
  microbenchmarks. They are not independently verified and are not load-bearing for any acceptance
  criterion here. EuleDB's own KPIs (AC-2 to AC-5) are measured on its own reference corpus.
- Some Lance capability will overlap with work planned in P1 and P2 (its own vector index against
  AC-31). That overlap is a saving to be measured, not a duplication to be avoided in advance.

## Alternatives considered

**Own columnar format.** Full control over layout and encryption placement, and the most instructive
option. Rejected: 3-5 person-months on the one layer that is not the product, plus an indefinite
maintenance tail for a single developer.

**Parquet + a separate vector index.** Mature and ubiquitous. Rejected: Parquet is optimised for scans,
not random access, and pairing it with an external index means two artifacts to keep consistent —
which contradicts the single-file goal.

**DuckDB as the storage engine.** Strong analytically, embedded, and would bring a SQL layer along.
Rejected: vector search is an add-on rather than a first-class index, and the non-goals explicitly
place analytical workloads outside this project.

**SQLite + an extension.** Maximum ubiquity. Rejected: row-oriented, so the columnar scans the hybrid
planner depends on would be fighting the engine.
