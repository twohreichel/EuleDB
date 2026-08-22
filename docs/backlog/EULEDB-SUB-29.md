---
id: EULEDB-SUB-29
ticket: EULEDB
fulfils: [AC-31]
depends_on: [EULEDB-SUB-28]
size: L
context_budget: 3000
safety: only a column declared auto-embedding behaves differently — every existing table is untouched
detail: stub
status: backlog
---

## Goal

**Keep an auto-embedding column consistent without a caller step.** A text column declared as auto-embedding is embedded on insert **and on update**, and its vector
index stays consistent with it without the caller doing anything.

The update path is the hard half and the one a test has to pin: a row whose text changes and whose vector
does not is a database that answers yesterday's question with today's confidence.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/schema.rs`, `definition.rs`, `store.rs`
- `docs/backlog/done/EULEDB-SUB-28.md`
- `docs/specs/spec.md (AC-31)`

## Notes for the cut

Declaration belongs on the schema here, unlike an index: which column carries embeddings is a
property of the table's shape, and a caller who inserts must not have to remember it. Chunking means one
row can produce several vectors — decide how they relate to the row and write it down before implementing,
because it decides what a hit even means.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
