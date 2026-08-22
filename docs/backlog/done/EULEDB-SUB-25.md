---
id: EULEDB-SUB-25
ticket: EULEDB
fulfils: [AC-29]
depends_on: [EULEDB-SUB-24]
size: L
context_budget: 3000
safety: the log is append-only and additive — no existing file format changes
detail: full
status: done
---

## Goal

**Append every operation to a hash-chained audit log.** One record per operation: what was asked, how it
resolved, and how many rows it affected. Each record carries the hash of its predecessor, so a removed or
altered entry does not go unnoticed.

## What landed

`AuditLog`, `AuditRecord`, `AuditError`, `LanceStore::audited`, and `Config::with_auditing` on the
facade. Create, insert, update, delete, drop and **scan** each leave one record.

The record carries what was asked, the row count, its predecessor's hash and its own. It does **not**
carry the rows: an audit log that copies the data it describes is a second copy of the database, and this
one is not encrypted. That absence has its own test, with distinctive row values so the assertion is real.

## Reads are recorded, and that is what needed the care

A recorded read is a write. AC-70 fixes many readers against one writer, so the log takes a short
exclusive lock on **its own file** — never the database's write lock. Readers serialise for the length of
one append and nothing else.

The sequence number and the predecessor hash are read **under** that lock. Deciding them before locking
is the classic read-then-write race, and it is the mutation that this ticket's first concurrency test
failed to catch.

Auditing is a tunable with its consequence stated where the knob is: off means no file at all, because an
audited handle cannot open a database on read-only media or one the caller may only read.

## The mutation pass found a vacuous test, which was the most useful result

Six mutations, four caught. **Two survived: removing the lock, and reading the tail before taking it.**
Twelve concurrent readers doing real database work never collided, so the concurrency test was green
either way — it proved that many readers can each record a read, which is a real claim, but not the one
its comment made.

Contention had to be manufactured: eight OS threads appending twelve records each, with nothing in
between. Both mutations are caught now, and the reader test's comment says what it does and does not
establish rather than letting the next reader assume.

A hostile predicate carrying tabs and newlines gets its own test: the line format escapes them, so a
predicate cannot forge a field boundary and insert a record of its own.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 132 tests
cargo +stable clippy --all-targets --all-features -- -D warnings
```

| Mutation | Caught by |
|---|---|
| the log takes no lock at all | `appends_from_many_threads_produce_a_gapless_chain` |
| the tail is read before the lock is taken | `appends_from_many_threads_produce_a_gapless_chain` |
| the predecessor hash is not carried, every record anchors | `every_operation_leaves_a_record_and_the_records_chain` |
| the free-form fields are not escaped | `a_predicate_carrying_the_separator_cannot_forge_a_record` |
| a read is not recorded | `a_read_only_handle_still_records_its_reads` |
| the delete records a row count of zero | `every_operation_leaves_a_record_and_the_records_chain` |

## A toolchain hazard worth the note

`just qa` failed with "no method named `audited`" for a method that plainly existed. `cargo publish
--dry-run --workspace` verifies the packaged facade while linking the storage crate out of the
**workspace** target directory, so a stale artefact there is compiled against the new packaged source. It
fires exactly when the facade starts using a storage API added in the same change. `cargo clean` resolves
it; the registry caches are innocent.

## Acceptance

- [x] AC-29 — one record per operation, reads included, carrying what was asked and how many rows it
      affected, each naming its predecessor and the first anchoring the chain.
- [x] AC-29 — the log takes its own lock on its own file, so AC-70's many readers are untouched, proven
      under manufactured contention.
- [x] AC-29 — auditing is a tunable on the one configuration mechanism, default on, with the consequence
      of switching it off stated where the knob is.
- [x] The log does not reproduce the rows it describes.
