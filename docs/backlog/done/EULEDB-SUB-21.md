---
id: EULEDB-SUB-21
ticket: EULEDB
fulfils: [AC-25]
depends_on: [EULEDB-SUB-20]
size: M
context_budget: 3000
safety: a new query shape beside the existing ones — nothing already working changes
detail: full
status: done
---

## Goal

**Answer a range predicate through the same index, in key order.** Ordering is the part worth testing
hardest: an index that returns the right rows in the wrong order satisfies a count assertion and still
breaks every caller that paginates.

## What landed

`Order`, `LanceStore::scan_ordered` and `LanceStore::scan_ordered_measured`. The read path that strips
this crate's encoding metadata off a batch's schema is now one shared helper, because `scan` and the two
new calls all need it and a read that forgot would hand a caller keys it never declared.

## The premise of AC-25 is wrong, and it was measured rather than assumed

The criterion is phrased so that a reader assumes the index supplies the order. **It does not.** A range
over an indexed column was run against both index kinds this format offers, and both returned storage
order — the fixture writes ids descending precisely so that key order and storage order are different
things, and an ordering bug cannot hide.

So the order is applied to the rows the predicate selected: a sort over the matches, not over the table.
That satisfies the criterion, and where the order comes from is written on the method rather than left
for a reader to infer.

**Measured, not argued.** A full scan followed by a sort returns exactly the same rows in exactly the
same order, so the two ordering tests cannot tell the implementations apart. `scan_ordered_measured`
exists for that one reason: it counts the rows the plan examined, and fewer than a tenth of the table is
what says the index narrowed before the sort.

## What this closes, and it closes it against the previous ticket

SUB-20 left the choice of index kind undefended and said so. It is now settled as **undefendable within
P1**: neither kind provides ordering, and both serve a range without a full scan. The ordered kind is
chosen on cardinality — a bitmap over a thousand distinct integers is a thousand bitmaps — and if a
low-cardinality column ever wants the other, that is a tunable for the mechanism of AC-74 rather than a
criterion. Recorded in the spec so the question is not re-opened from scratch.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 106 tests
cargo +stable clippy --all-targets --all-features -- -D warnings
```

Four mutations applied by hand, all caught:

| Mutation | Caught by |
|---|---|
| the direction is ignored, always ascending | `a_descending_range_comes_back_largest_first` |
| no ordering applied at all | `a_range_comes_back_in_key_order` |
| the predicate dropped, so the sort runs over everything | `an_ordered_range_still_goes_through_the_index` |
| the ordering column ignored, the key column used instead | `ordering_by_a_column_that_is_not_there_is_refused` |

## Acceptance

- [x] AC-25 — a range over an indexed column comes back in key order, ascending and descending, from a
      table that holds the rows in the opposite order. Expected values hand-written, not computed from
      the query.
- [x] AC-25 — "through the same index" measured: fewer than a tenth of the table examined, which a full
      scan followed by a sort would not achieve.
- [x] An ordering column that is not there is refused by variant, and the call joins the no-panic suite
      of AC-71.
- [x] An unindexed ordering column still answers correctly rather than refusing — a caller will do this
      sooner or later, and a correct answer at the price of a scan is the honest behaviour.
