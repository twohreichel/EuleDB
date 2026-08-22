---
id: EULEDB-SUB-21
ticket: EULEDB
fulfils: [AC-25]
depends_on: [EULEDB-SUB-20]
size: M
context_budget: 3000
safety: a new query shape beside the existing ones — nothing already working changes
detail: stub
status: backlog
---

## Goal

**Answer a range predicate through the same index, in key order.** A range predicate on an indexed column answered through the index built in SUB-20, returning rows
in key order. Ordering is the part worth testing hardest: an index that returns the right rows in the wrong
order satisfies a count assertion and still breaks every caller that paginates.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/store.rs`
- `docs/backlog/done/EULEDB-SUB-20.md`
- `docs/specs/spec.md (AC-25)`

## Notes for the cut

The format's scalar index expresses a range as a pair of bounds, each inclusive, exclusive or
unbounded. All nine combinations are one parametrised test, and the half-open cases are where an off-by-one
lives.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
