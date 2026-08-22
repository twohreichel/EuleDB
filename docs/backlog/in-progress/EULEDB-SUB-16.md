---
id: EULEDB-SUB-16
ticket: EULEDB
fulfils: [AC-69, AC-70]
depends_on: [EULEDB-SUB-15]
size: L
context_budget: 3000
safety: hardens existing paths, and makes a previously unenforced rule enforced
detail: full
status: in-progress
---

## Goal

An interrupted write leaves the database at the state before it or the state after it, **proven by
killing the writer rather than by argument**. And a concurrency model that is enforced: many readers, at
most one writer, a second writer refused with a distinct error.

## Context (read ONLY these files)

- `crates/euledb-storage/src/writer_lock.rs` (new), `src/store.rs`
- `crates/euledb-storage/tests/concurrency.rs` (new)
- `crates/euledb-crash-writer/` (new, unpublished)
- `docs/specs/spec.md` (AC-69, AC-70)

## The concurrency model, and why the lock is what it is

Many readers, at most one writer, per database directory. `new` opens for reading and takes no lock;
`open_for_writing` takes the write role and holds it until the store is dropped.

**An advisory lock on an open file handle, not a marker file.** The difference only shows on a crash: the
operating system releases the lock when the process dies, however it dies, whereas a marker file would
outlive the crash and lock the database out until somebody worked out which file to delete. There is a
test that kills a writer and then opens the same database for writing again.

**Refused immediately, never queued.** A local-first database that blocks forever on a lock held by a
process nobody can see is worse than one that says so. `std::fs::File::try_lock` reports contention as
its own error kind, so a busy database and a broken filesystem are not the same answer.

No dependency: the advisory lock has been in the standard library since 1.89, below the pinned minimum.

## What this changed everywhere else

`new` no longer writes. Every writing call site in the suite had to take the write role, which is the
enforcement working rather than a nuisance — the rule was previously a sentence in a specification.

It also surfaced a real API question the tests answered. `encrypted` **consumes** the store and carries
the write lock over, so rotating a key and continuing to write does not mean releasing the database and
racing another process for it. The rotation test now does exactly that, and says so.

## Proving crash safety

A fixture crate, `euledb-crash-writer`, `publish = false`: a process that opens the database for writing
and appends in a loop until something kills it. A simulated failure inside a test would only show that
the simulation agrees with itself.

Three tests kill it:

- **at five different points** — 15, 45, 90, 180 and 350 ms after the table exists — and each time the
  database must be readable and hold a **whole number of appends**. Each append is one commit, so a
  partial commit shows up as a row count that is not a multiple of the batch size.
- **and then open the same database for writing again**, which is the property that justifies an advisory
  lock over a marker file.
- **and twice in a row**, checking that whatever survived the first kill is still there after the second.
  Consistency alone is not the criterion — a completed write must not be among the losses.

### The gap in my own test

`0 % 250 == 0`. Every assertion above would have passed against a writer that never committed anything,
so the test also requires at least one run to have committed a row. Verified by breaking the fixture so
it never appends: the test fails. Without that line the suite would have reported crash safety it had not
observed.

## Two invariants added while here

The new crate is not published, so registry metadata is meaningless for it — but "no metadata" and
"deliberately not published" must not look the same. The metadata invariant now skips a crate that
declares `publish = false`, and a second invariant requires every member to be **either** publishable
**or** explicitly opted out. The middle ground is the one that gets discovered at a release tag.

And a third: **documentation that ships names no criterion id.** A `///` comment reaches docs.rs, where
`AC-70` is a reference the reader cannot follow. Six such references existed and are gone; ordinary `//`
comments may cite freely, and the test distinguishes them.

## Verification (executable)

```bash
just format && just lint && just test && just qa
cargo nextest run -p euledb-crash-writer     # the kills, about four seconds
cargo nextest run -p euledb-storage -E 'binary(concurrency)'
```

## Out of scope / Guardrails

- **Never make the write lock blocking.** Refusing immediately is the requirement, not a limitation.
- **Never replace it with a marker file**, however tempting for a friendlier message. It would survive a
  crash and lock the database out.
- **No lock for readers.** Unlimited readers is half the model, and taking a shared lock to read would
  make a local-first database unusable while anything writes.
- **No multi-process writing.** One writer means one, and a second is told so.

## Definition of Done

- [ ] AC-69 covered: killed at five points, whole commits only, readable each time, and the test proven
      non-vacuous
- [ ] AC-70 covered: a second writer refused by name, readers unblocked, a reader's write refused by name
- [ ] The model documented on the public API and on the crate's registry page
- [ ] A killed writer proven not to lock the database out
- [ ] The fixture crate explicitly unpublished, and the distinction made mechanical
