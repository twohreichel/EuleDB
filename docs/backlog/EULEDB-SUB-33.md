---
id: EULEDB-SUB-33
ticket: EULEDB
fulfils: [AC-37, AC-38, AC-39]
depends_on: [EULEDB-SUB-30, EULEDB-SUB-32]
size: L
context_budget: 3000
safety: fusion is a new call over two existing sources — neither changes
detail: stub
status: backlog
---

## Goal

**Fuse the two candidate lists, and say where each hit came from.** Reciprocal Rank Fusion over the vector and lexical lists, `score(d) = sum_r 1/(k + rank_r(d))`,
k = 60 by default, k in 10..20 below 100 documents with the effective k reported, and per-source ranks on
every fused hit.

This is where the exact filter of AC-27 becomes load-bearing: the pre-filter runs before either source
generates candidates, which the port already enforces.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/backlog/done/EULEDB-SUB-23.md` — the pre-filter port
- `docs/backlog/done/EULEDB-SUB-30.md`, `EULEDB-SUB-32.md`
- `docs/specs/spec.md (AC-37, AC-38, AC-39)`

## Notes for the cut

The expected scores are hand-computable from two short rank lists — do that rather than deriving
them from the implementation. A document found by both sources must score above one found by either alone
at the same rank, and that is the assertion that catches a fusion which merely concatenates.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
