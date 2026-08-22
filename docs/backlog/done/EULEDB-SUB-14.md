---
id: EULEDB-SUB-14
ticket: EULEDB
fulfils: [AC-23, AC-74]
depends_on: [EULEDB-SUB-17]
size: M
context_budget: 3000
safety: first published surface
detail: full
status: done
---

## Goal

Public crate API. Expose create-table, insert, scan, update, delete and drop as the documented Rust
API, with doc examples compiled and executed by the suite, and every tunable behind one documented
configuration mechanism. Python bindings stay out of scope until P5.

## What the repository actually looks like (checked, not assumed)

- `TableStore` carried create_table, append, scan, update and delete. **`drop_table` did not exist** —
  AC-23 names it, so it is new behaviour and goes through the cycle.
- **`euledb` does not depend on `euledb-storage`.** Its `lib.rs` says the crate is deliberately empty
  until the storage foundation exists, which it now does.
- The storage layer's boundary rule is that nothing outside it names the on-disk format. Re-exporting
  `LanceStore` under that name from the public crate would put the format in the published API, so the
  facade needs a type of its own.
- Tunables that exist today: compression per table on `TableDefinition`, and encryption per database
  through `LanceStore::encrypted`.

## Two pull requests, stacked

The whole ticket is over the ~400-line review limit, so it lands as two:

1. **`drop_table` in the storage layer** — the missing sixth operation, with its four behaviours.
2. **The facade** — the public type on `euledb`, the configuration mechanism, and doc examples that the
   suite runs.

## AC-74 — the decision, in writing

One configuration type carrying the *value* tunables, with a stated default and a stated effect for
each. A table may override the database default where the tunable is per table, and that override is
part of the documented mechanism rather than a second one.

The keyring stays out of it. A tunable is a knob with a default and an effect. A keyring has neither —
it is a credential, and putting it in a configuration struct would suggest there is a sensible default
for it. Encryption is therefore selected at open time, not configured.

Rejected: documenting `TableDefinition` and the open call together as "the mechanism". Two builders
described in one place are still two mechanisms, and the criterion exists so that later tunables have
one obvious home instead of growing private channels.

## Verification

```bash
just format && just lint && just test && just qa
```

## What landed

`euledb::Database` with the six operations, `open` / `open_for_writing` and their `_with` variants for a
configuration, and `encrypted` for a keyring. `euledb::Config` carries the tunables — today that is
compression, with its default and its effect stated on the method that sets it. The interchange types
are re-exported (`arrow_array`, `arrow_schema`), so a caller does not have to pin the same arrow major
by hand.

`insert` rather than `append`, because that is the word AC-23 uses. The layer below keeps `append`,
since that is what it does — rows are added, never merged — and the difference is documented on the
method rather than left to look like an inconsistency.

`Database` is partly a forwarding type, deliberately. What it earns: it keeps the on-disk format's name
out of the published API, it holds the configuration and applies it when a table is created, and it puts
the write role in the type system rather than in a flag. A new invariant test enforces the first of
those — using the store type inside the facade is fine, re-exporting it is not.

The per-crate README claimed to be "a placeholder at version 0.1.0" with an empty API. Both halves were
false after this change, and the version was already wrong. Both READMEs now say what works and, more
importantly, what the name promises but does not yet do.

## Acceptance

- [x] AC-23 — the six operations exposed through the `euledb` crate: `create_table`, `insert`, `scan`,
      `update`, `delete`, `drop_table`. Two doc examples compiled and executed by the suite.
- [x] AC-74 — one documented configuration mechanism, `Config`, with a stated default and a stated
      effect. Its effect is measured on disk, because a knob nobody can observe is decoration.
- [x] `drop_table` in the storage layer (PR #18), four behaviours, four mutations caught.
- [x] Five mutations on the facade, all caught: the configured compression never reaching the table,
      `with_compression` discarding its argument, a silent no-op `insert`, an `update` reporting a count
      without touching a row, and a no-op `encrypted`.
