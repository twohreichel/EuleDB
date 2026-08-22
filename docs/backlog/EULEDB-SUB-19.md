---
id: EULEDB-SUB-19
ticket: EULEDB
fulfils: []
depends_on: [EULEDB-SUB-14]
size: M
context_budget: 3000
safety: additive instrumentation — no existing path changes behaviour
detail: stub
status: backlog
---

## Goal

**Row identity and a measurable scan.** Surface the format's stable row identifier through the scan path, and count the rows a query
examined. Neither exists today, and both are preconditions for AC-24: it demands proof by an assertion on
rows examined rather than on wall-clock time, and there is nothing to assert on yet.

Fulfils no criterion of its own. It is the ticket that makes the next two testable at all.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/store.rs`
- `crates/euledb/src/database.rs`
- `docs/specs/spec.md (AC-24)`

## Notes for the cut

The format exposes row ids through its scanner. What has to be decided here is whether a row id
reaches the caller as a column on the batch or beside it, and whether the rows-examined count is per call
or cumulative on the handle. A cumulative counter on a shared handle is a mutable field on an otherwise
immutable type — prefer a value returned with the result.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
