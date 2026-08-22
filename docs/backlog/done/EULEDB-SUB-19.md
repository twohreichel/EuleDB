---
id: EULEDB-SUB-19
ticket: EULEDB
fulfils: []
depends_on: [EULEDB-SUB-14]
size: M
context_budget: 3000
safety: additive instrumentation — no existing path changes behaviour
detail: full
status: done
---

## Goal

**Row identity and a measurable scan.** Surface the format's stable row identifier, and count the rows an
operation had to look at. Fulfils no criterion of its own — it is what makes AC-24 assertable at all,
since that criterion demands proof on rows examined and there was nothing to assert on.

## What landed

`RowId`, `RowsExamined` and `Measured<T>` in `crates/euledb-storage/src/measurement.rs`, plus
`LanceStore::row_ids` and `LanceStore::row_ids_measured`.

`row_ids` answers a predicate with identities only — no data columns are projected, so a candidate set
does not cost a full row read. `row_ids_measured` answers the same question and reports how many rows the
widest step of the plan examined. It is a diagnostic and says so: measuring runs the plan a second time.

## The measurement, and what it can and cannot say

The engine exposes the number, but **renders it for human eyes**: a thousand rows arrive as `1.00 K`, so
1 000 and 1 004 cannot be told apart. This was found by running the plan and reading the output rather
than by assuming an integer, and it changed the design — the type documents the limit and the assertions
compare orders of magnitude, never exact counts at scale.

The number reported is the **widest** step, not the sum. A plan reads in stages, and a later stage
re-reading the handful of rows an earlier one selected has not examined them again in any sense a caller
cares about. The question is whether *some* step walked the whole table.

A plan that reports no such metric measures zero, deliberately: a missing metric is not evidence of a
narrow scan, and a zero fails an "examined the whole table" assertion loudly instead of passing a
"narrow" one quietly.

## The baseline this establishes

An exact lookup on a thousand-row table, with no index, examines all thousand rows. That number is
asserted now, bounded on both sides — the lower bound is the claim, the upper bound is what stops a
fabricated measurement from passing. It is the number SUB-20 has to beat, and asserting it here is what
makes the improvement visible rather than claimed.

## One dependency added

`lance-core`, pinned `=10.0.0` like the format itself, for the name it gives its row-id column. The
format does not re-export that constant, and naming the column the format names is what makes a rename
break the build instead of returning "no row-id column" at runtime. `lance-io` is a direct dependency
for the same reason.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 96 tests
```

Five mutations applied by hand, all caught:

| Mutation | Caught by |
|---|---|
| the first reported metric instead of the widest step | `the_widest_step_of_a_real_plan_is_the_full_table` |
| the sum of every step instead of the widest | `the_widest_step_of_a_real_plan_is_the_full_table` |
| a `K` suffix read as 1024 rather than 1000 | `a_rendered_count_reads_back_at_its_scale` |
| an unknown suffix guessed as one instead of refused | `an_unrecognised_rendering_is_refused_rather_than_guessed` |
| the predicate ignored, so every row comes back | `row_ids_identify_the_rows_a_predicate_matches` |

## Not covered

The empty projection is an optimisation no test verifies — `row_ids` would return the same identities
while reading every column. Verifying it needs the plan text, which no public call exposes. Stated rather
than left for a reader to assume.

## Acceptance

- [x] A stable row identity reaches a caller, is distinct within a table, and does not change between
      reads.
- [x] A predicate matching nothing yields no identity rather than a failure.
- [x] Rows examined is measurable, and the unindexed baseline is asserted on both sides.
- [x] Both new calls are covered by the no-panic suite of AC-71.
