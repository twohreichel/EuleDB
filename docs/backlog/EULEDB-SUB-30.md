---
id: EULEDB-SUB-30
ticket: EULEDB
fulfils: [AC-34]
depends_on: [EULEDB-SUB-29]
size: M
context_budget: 3000
safety: a new index kind beside the scalar one — nothing already indexed changes
detail: stub
status: backlog
---

## Goal

**HNSW as the vector index, with the documented defaults.** HNSW over an embedding column, M in 12..16, M0 = 2*M, cosine as the default distance, and a
recall assertion against the brute-force baseline from SUB-27.

The format ships HNSW and cosine, so this is expected to be its index rather than a new dependency —
the same decision as the scalar index in P1, with the same recorded cost of deeper coupling. Verify the
parameter names map onto AC-34's M and M0 before assuming they do.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/store.rs` — `create_index` is the shape to follow
- `docs/backlog/done/EULEDB-SUB-27.md` — the baseline to measure recall against
- `docs/specs/spec.md (AC-34)`

## Notes for the cut

It plugs into the `CandidateSource` port from SUB-23, which is why that port exists. Recall is
the assertion, not "a result came back" — an index that returns plausible neighbours and misses the
nearest one passes every shape check.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
