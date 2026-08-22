---
id: EULEDB-SUB-31
ticket: EULEDB
fulfils: [AC-35]
depends_on: [EULEDB-SUB-30]
size: M
context_budget: 3000
safety: a second index kind behind the same query API — a table using the first is unaffected
detail: stub
status: backlog
---

## Goal

**IVF-PQ where memory is constrained, selectable per table.** IVF-PQ as an alternative vector index, chosen per table, **without changing the query API**. That
last clause is the criterion's substance: the caller's query must not mention which index answers it.

The measurable difference is resident memory, and AC-4's ceilings are what make the choice meaningful. A
test that only shows both kinds return rows has not tested the reason the second one exists.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/backlog/done/EULEDB-SUB-30.md`
- `docs/specs/spec.md (AC-35, AC-4)`

## Notes for the cut

The format offers `IvfPq` and `IvfHnswPq`. Which one AC-35 means is worth settling in writing —
they are different trade-offs, and the criterion names only the first.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
