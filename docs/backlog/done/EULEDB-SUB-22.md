---
id: EULEDB-SUB-22
ticket: EULEDB
fulfils: [AC-26]
depends_on: [EULEDB-SUB-19]
size: M
context_budget: 3000
safety: a pure computation over row-id sets, reachable only through a new call
detail: full
status: done
---

## Goal

**Evaluate conjunctive and disjunctive predicates as Roaring set operations**, and prove the result
equals the equivalent filter over the same data.

## What landed

`RowIdSet`, `LanceStore::row_ids_all` and `LanceStore::row_ids_any`. Each predicate is answered on its
own and the answers are combined as compressed bitmaps, so a conjunction costs one narrow read per part
rather than one pass evaluating all of them. Where an index covers a part, that part is served by it.

`RowIdSet` is a type of ours over the bitmap library's, because returning the library's type would put it
in this crate's public API and make it permanent — the same reason the on-disk format's types do not
leave this crate.

## The independent reference

Two of them, and they check different things.

The **expected counts are hand-read off the corpus**: ids 40..70 is thirty rows, every third row is
German, so the conjunction is ten. `de` is 334 of a thousand and 34 of the first hundred, so the union of
"first hundred" and "German" is 400. None of those numbers is computed by the code under test.

The **set itself is compared against the same question asked as one expression**. Intersecting two
result sets and letting the engine evaluate `a AND b` are genuinely different paths, so agreement is
evidence rather than a tautology.

## What no other assertion would have caught

Every other test in the file would still pass if `all` united and `any` intersected, as long as it did so
consistently. `the_same_parts_combined_the_two_ways_give_different_answers` is the one that tells the
operators apart — 34 against 400 — and it also asserts the relation that holds for any inputs: every row
of the conjunction is in the disjunction.

## An unreachable branch, removed rather than left defensive

`intersect_all` first had an empty case returning the empty set. It was unreachable: the caller refuses an
empty predicate list, which the refusal test proves. The functions now take the first set apart from the
rest, so there is no empty case to answer for. An intersection of nothing has no answer a set can carry
anyway — the identity is the universe, which is not enumerable.

An empty list is refused rather than defaulted, in both directions: an empty conjunction is every row and
an empty disjunction is none, so either default silently answers a question the caller did not ask. The
refusal names which operation was attempted, and a mutation making both refusals claim the same one is
caught.

## No property-based test, and why

The ticket's notes suggested one — commutativity, associativity, De Morgan. Those hold by construction of
the bitmap library, so a property test over them would test `roaring`, which has its own suite. The
properties worth asserting here are about *our* combination logic, and they are asserted directly.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 111 tests
cargo +stable clippy --all-targets --all-features -- -D warnings
```

Five mutations applied by hand, all caught:

| Mutation | Caught by |
|---|---|
| a conjunction unites instead of intersecting | `the_same_parts_combined_the_two_ways_give_different_answers` |
| a disjunction intersects instead of uniting | `the_same_parts_combined_the_two_ways_give_different_answers` |
| every predicate after the first is dropped | `a_conjunction_is_the_intersection_of_its_parts` |
| an empty list answers with the empty set instead of refusing | `combining_no_predicates_at_all_is_refused` |
| both refusals claim the same operation | `combining_no_predicates_at_all_is_refused` |

## Acceptance

- [x] AC-26 — conjunction and disjunction evaluated as bitmap intersection and union, each equal to the
      same question asked as one expression, with counts hand-read off the corpus.
- [x] One predicate combines to itself, in both directions.
- [x] An empty list is refused by variant, naming the operation.
- [x] The new call joins the no-panic suite of AC-71.
