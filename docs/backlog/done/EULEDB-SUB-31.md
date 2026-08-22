---
id: EULEDB-SUB-31
ticket: EULEDB
fulfils: [AC-35]
depends_on: [EULEDB-SUB-30]
size: M
context_budget: 3000
safety: a second index kind behind the same query API — a table using the first is unaffected
detail: full
status: done
---

## Goal

**IVF-PQ where memory is constrained, selectable per table, without changing the query API.** That last
clause is the criterion's substance: the caller's query must not mention which index answers it.

## What landed

`VectorIndexKind` with `Graph` and `Quantised`, a kind parameter on `create_vector_index`, and
`vector_index_kind` to read back what a column carries. `nearest` is unchanged — the same call serves both.

## Three assumptions of mine, all measured and all wrong

**"The quantised index is smaller."** It is not, at this scale: **27 847 bytes against the graph's
16 479** over two dozen vectors. The codebook is a fixed cost and dominates until the collection is far
larger than it. A test asserting the direction would assert something untrue, so the direction is not
asserted — the crossover is a memory measurement over the reference corpus, not a unit test.

**"Both kinds find the same nearest vector."** They do not — the graph returned `[4, 5, 14]` and the
quantised index `[5, 4, 2]`. Product quantisation answers from a lossy code, and with a four-bit codebook
over two dozen vectors the loss is large. The test now asserts what the criterion actually says: the same
call serves both.

**"Comparing artefact sizes shows which kind was built."** It does not. Two builds of the *same* kind
differ in bytes, so `assert_ne!` on sizes is satisfied by noise — a mutation ignoring the requested kind
survived it. The index's own recorded type is the only reliable signal.

## What the mutation pass forced into the design

`vector_index_kind` exists because of that last finding. Nothing else could distinguish the two kinds:
not the answers, not the artefacts. And it is the right API to have anyway — "selectable per table" is a
real property only if a caller can find out what was selected.

The quantiser's parameters are sixteen sub-vectors at four bits rather than the defaults, because
**product quantisation cannot train a codebook on fewer vectors than it has centroids**. Twelve documents
failed with exactly that message. The test corpus is twenty-four documents for the same reason, and the
constant says so.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 167 tests
```

| Mutation | Caught by |
|---|---|
| the requested kind is ignored, always the graph | `the_index_records_the_kind_it_was_asked_for`, added for it after two weaker attempts failed |
| the codebook is too large to train at this scale | `either_index_kind_answers_the_same_query_call` |

## Acceptance

- [x] AC-35 — both index kinds are buildable, and the query API does not change with the choice.
- [x] AC-35 — the selection is observable, which is what makes it a selection rather than a coin toss.
- [x] The reason the second kind exists is *not* claimed at a scale where it is false. Recorded instead.
