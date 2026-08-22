---
id: EULEDB-SUB-20
ticket: EULEDB
fulfils: [AC-24]
depends_on: [EULEDB-SUB-19]
size: L
context_budget: 3000
safety: a table with no index behaves exactly as it did — indexing is an explicit call
detail: full
status: done
---

## Goal

**Declare an indexed column, and answer a point lookup without a full scan.** The assertion is on rows
examined: a lookup on an indexed column must examine a number of rows proportional to the matches rather
than to the size of the table.

## What landed

`LanceStore::create_index(table, column)`, and the measurement SUB-19 built proves the claim: the same
lookup that examined all 1 000 rows now examines fewer than 10.

## The open design question, answered by the format rather than by preference

The cut asked where an index declaration lives — on the schema, so it travels with the table, or on the
definition beside the compression. **Neither.** The format builds an index over rows that already exist,
so an index cannot be an attribute of a table declared before any row does. It is an operation, the way
`CREATE INDEX` is an operation in every other database.

The second question was what happens to rows appended afterwards, and probing answered it: they are
**found, correctly**, by scanning the part the index does not cover. A lookup after appending 100 rows to
an indexed 1 000 examines about 100 — not 1 100, and not a handful either. Calling `create_index` again
rebuilds over everything and returns the lookup to a handful, which is what makes the append cost
recoverable rather than permanent. That is the surprising half of the behaviour, so it has its own test.

## What no test here defends, stated plainly

The index kind is `BTree` and **no test in this ticket distinguishes it from `Bitmap`**. A mutation
swapping them survives every assertion, including the range one written specifically to catch it: in this
format a bitmap index serves a range without a full scan too. The choice rests on cardinality instead — a
bitmap over a thousand distinct integers is a thousand bitmaps — and it becomes behaviour a test can see
only once ordering is exercised, which is AC-25 and SUB-21.

## One dependency added

`lance-index`, pinned `=10.0.0` like the rest. The format re-exports the trait that creates an index but
not the enum naming which kind, and the enum has to be the same version as the format or it is a
different enum. Third format crate as a direct dependency, each for a type the facade crate does not
re-export. The boundary still holds: none of them leaves this crate.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 101 tests
cargo +stable clippy --all-targets --all-features -- -D warnings
```

Four mutations applied by hand, three caught and one recorded as surviving:

| Mutation | Caught by |
|---|---|
| the write role is not required to build an index | `a_reader_cannot_create_an_index` |
| `replace` turned off, so a rebuild refuses | `rows_appended_after_the_index_are_found_by_scanning_only_the_remainder` |
| the index is reported as built without being built | `an_indexed_lookup_examines_a_handful_of_rows` |
| **a bitmap index instead of the ordered one** | **nothing — see above** |

## Acceptance

- [x] AC-24 — a point lookup on an indexed column examines fewer than 10 rows of 1 000, and still
      returns the right row. The unindexed baseline of 1 000 is asserted in SUB-19, so the improvement
      is measured rather than claimed.
- [x] A column that is not there, and a reader attempting to index, are both refused by variant.
- [x] The new call joins the no-panic suite of AC-71.
