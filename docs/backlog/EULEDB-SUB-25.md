---
id: EULEDB-SUB-25
ticket: EULEDB
fulfils: [AC-29]
depends_on: [EULEDB-SUB-24]
size: L
context_budget: 3000
safety: the log is append-only and additive — no existing file format changes
detail: stub
status: backlog
---

## Goal

**Append every operation to a hash-chained audit log.** One record per operation: what was asked, the plan that resolved it, and how many rows it affected.
Each record carries the hash of its predecessor, so a removed or altered entry cannot go unnoticed.

**Readers append too**, by decision, which is the part that needs care: AC-70 fixes many readers and one
writer, so the log gets its own short-lived exclusive lock on its own file. Readers serialise for the
duration of one append and nothing else. Auditing is a documented tunable on `Config` — off means a database
on read-only media stays readable, and the consequence is stated where the knob is.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/writer_lock.rs`
- `crates/euledb/src/config.rs`
- `docs/backlog/done/EULEDB-SUB-24.md`
- `docs/specs/spec.md (AC-29, AC-70)`

## Notes for the cut

The record is a value with a schema, not a formatted string — AC-30 has to verify a chain over it,
and a log nobody can parse cannot be verified. Do not log the row data: an audit log that copies the rows it
describes is a second, unencrypted copy of the database.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
