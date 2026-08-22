---
id: EULEDB-SUB-23
ticket: EULEDB
fulfils: [AC-27]
depends_on: [EULEDB-SUB-22]
size: M
context_budget: 3000
safety: a port with no real implementation yet — the fake in the test is its only consumer
detail: full
status: done
---

## Goal

**Apply the exact filter as a pre-filter before candidate generation.** A query carrying both an exact
filter and a search clause must narrow by the filter first and hand the surviving row ids onward, not the
other way round.

## What landed

`CandidateSource`, a driven port, and `LanceStore::filtered_search`. The filter runs first and its result
is handed over, so ranking never considers a row the caller excluded and never spends work doing so.

**Why the order matters, stated on the method:** generating candidates first and filtering afterwards
returns the same rows for a small table and the *wrong* ones as soon as a limit truncates the candidates
before the filter has had its say. That is why this is a criterion rather than an optimisation.

## A port with no implementation, on purpose

There is no vector or full-text searcher until P2. What lands is the seam, and it is justified at zero
implementations because it inverts the dependency arrow rather than abstracting over variants: the query
path decides that filtering happens first, and a searcher plugged in later cannot change that by accident.

The test uses a **hand-written fake, not a mock**. A mock asserting that a call happened would prove
nothing about the order of operations, which is the entire content of the criterion. The fake records the
set it was handed, and its cardinality is the evidence — ten rows rather than the thousand it did not see.

## Two decisions worth their line

**An empty filter is refused**, as combining no predicates is. A search with no filter needs no pre-filter
and should ask the source directly.

**A filter matching nothing still calls the source**, with an empty set. Short-circuiting would be
correct — the answer is empty either way — and is deliberately not done: a special case in the query path
is a branch to maintain and test for no gain, and the port's contract already says an implementation draws
only from what it is given. A mutation adding the short-circuit is caught, so the choice is pinned rather
than incidental.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 115 tests
cargo +stable clippy --all-targets --all-features -- -D warnings
```

Four mutations applied by hand, all caught:

| Mutation | Caught by |
|---|---|
| candidates generated first, filtered afterwards | `candidate_generation_sees_only_the_rows_the_filter_kept` |
| the limit never reaches the source | `candidate_generation_sees_only_the_rows_the_filter_kept` |
| the source's failure is swallowed and the filter's rows returned | `a_failing_source_fails_the_search` |
| an empty filter result short-circuits past the source | `a_filter_matching_nothing_hands_over_an_empty_set` |

## Acceptance

- [x] AC-27 — candidate generation receives exactly the rows the filter kept, asserted by cardinality
      read off the corpus rather than by a call having happened.
- [x] The limit reaches the source, and every candidate comes from the narrowed set.
- [x] A source's own failure passes through unchanged rather than being replaced by the filter's rows.
- [x] An empty filter is refused and never reaches the source.
