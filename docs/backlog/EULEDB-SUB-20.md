---
id: EULEDB-SUB-20
ticket: EULEDB
fulfils: [AC-24]
depends_on: [EULEDB-SUB-19]
size: L
context_budget: 3000
safety: a table with no declared index behaves exactly as it does today
detail: stub
status: backlog
---

## Goal

**Declare an indexed column, and answer a point lookup without a full scan.** Declare which columns carry an index, build it, and answer an exact lookup through it. The
assertion is on rows examined: a lookup on an indexed column must examine a number of rows proportional to
the matches, not to the size of the table.

Uses the format's persisted scalar index rather than a hand-built Adaptive Radix Tree — see the decision
recorded in § Technology stack. The index is persisted and encrypted through the existing object store,
so neither costs work here.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/store.rs`
- `crates/euledb-storage/src/definition.rs`
- `crates/euledb-storage/src/schema.rs`
- `docs/specs/spec.md (AC-24)`

## Notes for the cut

Two design questions to settle in writing before implementing: where an index declaration lives
(on the schema, so it travels with the table, or on the definition beside the compression) and what happens
when a column is declared indexed after rows already exist. The second is the one a reviewer will ask about.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
