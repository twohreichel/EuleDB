---
id: EULEDB-SUB-30
ticket: EULEDB
fulfils: [AC-34]
depends_on: [EULEDB-SUB-29]
size: M
context_budget: 3000
safety: a new index kind beside the scalar one — nothing already indexed changes
detail: full
status: done
---

## Goal

**HNSW as the vector index, with the documented defaults**, and a recall assertion against an exhaustive
baseline.

## What landed

`LanceStore::create_vector_index`, `nearest`, and `nearest_uses_the_index`. The index is the format's HNSW
with `m = 16` and cosine over one IVF partition.

## What the criterion asks for and what the format offers

Two of three are honoured — `m = 16` and cosine. **`M0 = 2*M` cannot be:** this format offers HNSW only
*inside* an IVF partitioning, and its build parameters are `max_level`, `m`, `ef_construction` and a
prefetch distance, with no distinct connectivity for the bottom layer. Recorded in the spec rather than
quietly dropped.

One IVF partition, because this is the small-and-mid-size case the criterion names: partitioning would
push a query into the wrong partition more often than it would save.

## Three mutations survived, and each said something different

**The index is never built — every neighbour assertion still passed.** With a small collection an
exhaustive comparison returns exactly what the index returns. So the answers are not evidence that an
index exists, and `nearest_uses_the_index` reads the plan instead. Same lesson as the scalar index: for
anything whose point is *how* an answer was reached, the answer cannot be the proof.

**Cosine swapped for L2 survived, and always will.** Every stored vector is L2-normalised, and for unit
vectors squared Euclidean distance is `2 - 2·cosine` — strictly decreasing in it, so the ranking is
identical. That is arithmetic, not a gap in the tests. Cosine is set because it is the correct label for
normalised vectors, and the spec now says so.

**The query-width check was untested**, and fixing it exposed a design mistake of mine: a wrong-width
query was reported as a `Backend` failure, which means "the layer below refused" when nothing below was
asked anything. It has its own variant now, and the message carries both numbers — the difference between
something a caller can act on and "could not search".

## What my own invariant caught

`documentation_that_ships_names_no_criterion_id` failed on the first doc comment I wrote for
`create_vector_index`: it cited a criterion id that a reader of the published documentation cannot
resolve. Rewritten to state the substance instead. The invariant earned its place.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 164 tests
```

| Mutation | Caught by |
|---|---|
| the limit does not reach the search | `an_indexed_vector_column_finds_what_an_exhaustive_search_finds` |
| `m` collapsed to 1 | `an_indexed_vector_column_finds_what_an_exhaustive_search_finds` |
| the query width is not checked | `a_query_of_the_wrong_width_is_refused`, added for it |
| the index is never built | `the_search_goes_through_the_index_once_one_exists`, added for it |
| cosine swapped for L2 | nothing, and nothing can — see above |

## Acceptance

- [x] AC-34 — an indexed nearest-neighbour search agrees with an exhaustive one on at least four of five,
      and finds the nearest vector exactly, with the baseline computed in the test rather than stored.
- [x] AC-34 — `m` inside the documented range and cosine as the distance. `M0` is not expressible and is
      recorded as such.
- [x] The search demonstrably goes through the index once one exists, read from the plan.
- [x] A query of the wrong width is refused by its own variant, naming both widths.
