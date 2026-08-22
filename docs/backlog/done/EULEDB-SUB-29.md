---
id: EULEDB-SUB-29
ticket: EULEDB
fulfils: [AC-31]
depends_on: [EULEDB-SUB-28]
size: L
context_budget: 3000
safety: only a column declared auto-embedding behaves differently — every existing table is untouched
detail: full
status: done
---

## Goal

**Keep an auto-embedding column consistent without a caller step.** A text column declared as
auto-embedding is embedded on insert **and on update**, and its vectors stay consistent with it.

## What landed

`TableSchema::auto_embedding(column)`, an `Embedder` port in the storage layer, `LanceStore::embedding`,
and `vectors_of` to read the result. Insert, update and delete all reconcile.

**The declaration lives on the schema and is persisted as field metadata**, exactly like the compression
setting. It has to survive the handle that declared it: a second process opening the same table must not
be able to forget that a column embeds itself.

**The port points inwards.** The storage layer owns the trait and decides *when* text is embedded; the
adapter in `euledb-embed` decides *how* and therefore depends on the storage layer, not the other way
round. That is what keeps 200 crates and 470 MB of weights out of the storage layer's dependency graph.

## Reconciliation, not incremental bookkeeping — and the measurement that forced it

Two of these tests failed on the first run, and the cause was the same for both: **an update gives the
row a new row id.** The format rewrites the row into a new fragment rather than editing it in place, so
row 0 came back as row 4294967296.

That single fact settles the design. Anything keyed on row identity is stale after an update, so patching
a vector table incrementally would need to track identity changes the format does not announce. One rule
covers insert, update and delete instead: vectors whose row is gone are dropped, rows whose text has no
vector are embedded. It is self-healing, so a write interrupted half way leaves nothing permanently wrong.

It also has its own test now, `an_updated_row_comes_back_with_a_new_identity`, so the next person to build
an index over row ids finds the fact instead of rediscovering it.

**The cost, stated:** one scan of the table per write. That is the price of correctness here and it is the
thing to replace when it starts to mattering — not the correctness.

## Two things that are not observable, said rather than asserted

**Whether an unchanged text was re-embedded.** Re-embedding produces byte-identical output, and the row
id changed anyway, so the test asserts what a caller can actually rely on: the vectors still describe the
same texts. Naming it avoids a future reader believing the test proves work was skipped.

**A missing embedder is refused, not skipped.** A row stored with no vector is a row no semantic query
can ever find, and silence there would be a database quietly forgetting half of what it was given.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 160 tests
cargo +stable clippy --all-targets --all-features -- -D warnings
```

Five mutations applied by hand, all caught:

| Mutation | Caught by |
|---|---|
| an update does not reconcile | `updating_the_text_re_embeds_that_row` |
| a delete leaves orphaned vectors | `deleting_a_row_takes_its_vector_with_it` |
| a missing embedder is silently skipped | `a_handle_without_an_embedder_refuses_to_insert` |
| the auto-embedding declaration is ignored | `inserting_a_row_embeds_its_text_without_being_asked` |
| a wrong-width vector is accepted | `inserting_a_row_embeds_its_text_without_being_asked` |

## Acceptance

- [x] AC-31 — a declared column embeds on insert and on update, with no caller step, and the declaration
      survives the handle that made it.
- [x] Vectors are attributable to their rows, with a chunk index, so a hit resolves back to data.
- [x] A delete takes its vectors with it — an orphaned vector is a hit that resolves to nothing.
- [x] An auto-embedding table refuses an insert from a handle that cannot embed.
