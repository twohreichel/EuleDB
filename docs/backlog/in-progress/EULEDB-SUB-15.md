---
id: EULEDB-SUB-15
ticket: EULEDB
fulfils: [AC-67, AC-68]
depends_on: [EULEDB-SUB-13]
size: L
context_budget: 3000
safety: additive API — the existing insert and scan paths are unchanged
detail: full
status: in-progress
---

## Goal

Update rows matching a predicate and leave every other row alone. Delete rows matching a predicate,
report how many, and **log the count and the predicate before executing** — a delete broader than
intended has to be visible in the log rather than inferred later from missing data.

## Context (read ONLY these files)

- `crates/euledb-storage/src/mutation.rs` (new), `src/store.rs`
- `crates/euledb-storage/tests/mutations.rs` (new)
- `docs/specs/spec.md` (AC-67, AC-68)

## Design

`Predicate` and `Assignment` are newtypes over strings rather than bare `&str`. Two reasons, and the
second is the load-bearing one:

- A bare string is an invitation to pass a user's text and discover later that it was evaluated.
- **The eventual validated query representation needs somewhere to land.** When the query layer arrives,
  it produces a `Predicate`, and every call site already takes one. A `&str` parameter would have to
  change everywhere.

Nothing here validates the expression. The storage layer refuses one it cannot evaluate, and that is
the behaviour worth having: **an unknown column is an error, not a filter that quietly matches nothing.**
There is a test for exactly that, because the alternative — a delete that silently matches nothing, or
the wrong rows — is the failure mode this criterion exists to prevent.

`update` refuses an empty assignment list with its own error. The layer below refuses it too, so the
guard is about the *message*: a caller who forgot the assignments should be told that, not handed
something about a query plan.

## The announcement, and why the count comes first

AC-68 requires the count and the predicate to be logged **before** the delete runs, so the delete counts
matching rows first, logs, and only then deletes. That costs an extra pass and it is the point: the
number has to be visible at the moment it is about to happen.

At **WARNING**, not INFO. Removing rows degrades what the database holds, and an operator who reads only
warnings still has to see it. The count, the predicate and the table are **named fields**, not
interpolated into the message, so the message stays groupable and the values stay machine-readable.

`tracing` rather than `log`: this layer is async, the storage format already emits tracing spans so an
application here almost certainly has a subscriber, and named fields are what an announcement like this
needs. An application that only has a `log` backend can bridge it.

## TDD record, and the three things the mutation pass changed

Eight tests, written first, all RED on the missing types. Then the mutation pass:

| Mutation | Caught | |
|---|---|---|
| the announcement reports zero | yes | |
| the announcement omits the predicate | yes | |
| the announcement drops below warning | yes | |
| the update ignores its predicate | yes | |
| **an update with nothing to set is accepted** | **no** | the test asserted only `is_err()`, and the layer below refuses it too. Now it asserts the *specific* error, so removing the guard fails |

And one finding that removed code rather than adding it. `Deleted` originally carried a second field,
`announced`, holding the count that was logged — so a caller could check the log against reality. **It
can never differ.** At most one writer may hold a database (AC-70), so nothing changes the table between
the count and the delete, and no mutation of that field could be caught by any test. A field that can
never differ is surface without a claim, so it is gone. The logged count is asserted where it belongs:
against the log.

The delete-count test was rewritten at the same time. It compared the returned count against the
returned count. It now compares it against the rows that actually left the table — a number that does not
come from the same call.

## Verification (executable)

```bash
just format && just lint && just test && just qa
cargo nextest run -p euledb-storage -E 'binary(mutations)'
```

Eight tests: an update touching only matching rows and surviving a reopen; a delete removing exactly the
matching rows; a delete matching nothing changing nothing; an unknown column refused rather than matching
nothing; the count checked against rows that actually left; an empty assignment list refused by name; the
announcement captured from a `tracing` subscriber and checked for the level, the count, the predicate and
the table; and update plus delete over an **encrypted** table, because a mutation rewrites fragments and
commits a manifest, exercising the encrypting layer's write path as well as its read path.

## Out of scope / Guardrails

- **Never log the announcement after the delete.** The whole requirement is the ordering.
- **Never let an unparseable predicate become "matches nothing".** That is a silent wrong answer, and for
  a delete it is the worst possible one.
- **No `drop`** — AC-23 in SUB-14 owns the public surface, and dropping a table is a different kind of
  destruction.
- Do not add a field to a result that cannot differ from another one. It reads as a safety check and is
  not one.

## Definition of Done

- [ ] AC-67 covered: matching rows updated, non-matching untouched, values present after a reopen
- [ ] AC-68 covered: exactly the matching rows removed, count reported, count and predicate logged first
- [ ] The announcement asserted from a real subscriber, not assumed
- [ ] Every mutation of the path either caught or shown to be semantically equivalent
- [ ] Mutations proven to work on an encrypted table as well as a plaintext one
