---
id: EULEDB-SUB-22
ticket: EULEDB
fulfils: [AC-26]
depends_on: [EULEDB-SUB-19]
size: M
context_budget: 3000
safety: a pure computation over row-id sets, reachable only through a new call
detail: stub
status: backlog
---

## Goal

**Evaluate conjunctive and disjunctive predicates as Roaring set operations.** Combine per-predicate row-id sets with Roaring bitmap intersection and union, and prove the result
equals a brute-force filter over the same data. The brute-force pass is the independent reference the
criterion asks for, and it is what stops the test from checking the implementation against itself.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/mutation.rs`
- `docs/backlog/done/EULEDB-SUB-19.md`
- `docs/specs/spec.md (AC-26)`

## Notes for the cut

`roaring` is `set` in § Technology stack, so there is nothing to evaluate. Worth a property-based
test rather than examples alone: intersection and union are commutative and associative, and De Morgan
relates them — those hold for every input, which is exactly what a property test is for.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
