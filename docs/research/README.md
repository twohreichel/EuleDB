# Research record

One document, kept for provenance rather than for reading: the technical feasibility study and name
analysis that the specification was derived from, written before any code existed.

| | |
|---|---|
| File | [`research-report-hybrid-database.md`](research-report-hybrid-database.md) |
| Written | 2026-08 |
| Language | German |
| Working name at the time | **SemanticDB** — the name analysis in the second half is what led to *EuleDB* |

**It is not maintained and it is not authoritative.** Where it disagrees with
[`../specs/spec.md`](../specs/spec.md), the specification wins — several of the report's conclusions
were deliberately overruled while the spec was written, and those reversals are recorded in the spec
under § Decisions taken. Two examples: the report put a Python binding skeleton in the first phase
(deferred to P5), and it anchored the performance targets to two named machines (replaced by
capability-defined platform classes).

Read it to understand *why* a choice was made. Read the specification to find out *what* is being built.
