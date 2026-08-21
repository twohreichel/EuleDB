---
id: EULEDB-P2
ticket: EULEDB
kind: phase
fulfils: [AC-2, AC-3, AC-4, AC-5, AC-31, AC-32, AC-33, AC-34, AC-35, AC-36, AC-37, AC-38, AC-39, AC-64, AC-72]
depends_on: [EULEDB-P1]
size: epic
estimate_pm: "3-4"
context_budget: 2000
safety: not a mergeable unit — see below
detail: stub
status: backlog
---

## Goal

**P2 — Semantics and full text.** The three retrieval paths become one hybrid query, and the KPIs stop being aspirational because there is finally something to measure. **The first public release follows this phase.**

## Effort

**3-4 person-months** for one experienced developer, per the research estimate (concept § 5). The
total across P0-P5 is 17-21. The number assumes the chosen crates hold up and the UX scope does not
grow — treat a large overrun as a signal to re-cut, not to work longer.

## This is a phase, not a ticket

`size: epic
estimate_pm: "3-4"` is deliberate and it is a warning, not a label. This exceeds `L`, so it is **not
executable as one session and must not be started as one.** When P2 becomes next, cut it into
`EULEDB-SUB-<n>` tickets of size S to L before any work starts, then work those one at a
time. This file exists so the criteria below are visible in the backlog instead of being remembered.

## Criteria in scope

- embedding pipeline: chunking, E5 prefix convention, L2 normalisation (AC-32)
- 384-dimensional deterministic embeddings via `multilingual-e5-small` (AC-33)
- auto-embedding columns kept consistent with their vector index (AC-31)
- HNSW and IVF-PQ, selectable per table (AC-34, AC-35)
- Tantivy BM25 with stable ranking (AC-36)
- RRF fusion, k = 60, small-corpus k reported (AC-37, AC-38)
- per-source ranks on the fused result (AC-39)
- reference corpus chosen, benchmark harness, KPIs published (AC-2, AC-3, AC-4, AC-5)
- community metrics published beside the technical ones — they gate the P3 decision (AC-64)
- **getting-started guide, release-blocking**: install, create a table, insert, one query of each
  kind plus a hybrid one, every example compiled and executed by the suite (AC-72)

## Notes for the cut

Two decisions land here, both currently open: `ort` versus `candle` as the ONNX runtime (now spans four CI platforms, consequential for aarch64 cross-compilation), and whether Lance's own vector index removes the need for a separate HNSW crate. Measure before adding a dependency. The reference corpus must be fixed BEFORE the first benchmark is recorded, or AC-5 is unverifiable.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this phase is cut. A ticket
detailed today against a repository state an earlier phase will change is wrong by the time it is
picked up — which is exactly why this file stays coarse.
