---
id: EULEDB-SUB-36
ticket: EULEDB
fulfils: [AC-72]
depends_on: [EULEDB-SUB-33]
size: M
context_budget: 3000
safety: documentation, with its examples compiled by the suite
detail: full
status: done
---

## Goal

**A getting-started guide a newcomer can actually follow.** Install, declare a table, insert, and run each
of the three query kinds — exact filter, semantic, full text — plus one hybrid query, with every example
compiled and executed by the test suite.

## What landed

`docs/getting-started.md`, and the API it needed in order to be followable. The guide was the honest test
of the surface, and the surface failed it twice.

**The facade could not answer two of the three questions.** `Database` had no way to build either index and
no way to search by meaning or by words — every P2 capability existed on `LanceStore`, which is the layer
the facade exists to hide. `index_vectors`, `index_text`, `semantic_search`, `text_search` and
`hybrid_search` are the guide's requirements turned into methods.

**The embedding crate was unpublished.** A guide that has a newcomer construct an embedder cannot be
followed from a registry the embedder is not on, so `euledb-embed` is now published — with its own
description, keywords, categories and README, and its tests excluded, because their fixture is a slice of a
CC BY-SA corpus and has no business inside an Apache-2.0-or-MIT package.

## The query prefix is a port method, not a flag

E5 embeds a query and a stored passage under different prefixes, and using the wrong one costs recall. So
`Embedder` gained `embed_query` beside `embed_passage` rather than a boolean: two names cannot be confused
at a call site, and a mistake in a flag would be invisible in every test that only checked a vector came
back.

Embedding a query then belongs on the store, not the facade — the handle already holds the embedder for the
auto-embedding path, so the read side uses the same knowledge instead of a second copy of it. The first
version kept an `Arc` on `Database` as well and the review round removed it: two owners of one thing is one
that can be forgotten by a future constructor.

## The mutation pass found four real gaps, and three had the same shape

| Mutation | Caught by |
|---|---|
| the port embeds a query as a passage | **nothing** → `the_port_embeds_a_query_as_a_query_and_not_as_a_passage` |
| a database with no embedder answers with a zero vector | `a_semantic_query_without_an_embedder_says_what_is_missing` |
| `semantic_search` widens the limit it was given | `a_newcomer_can_run_every_query_kind` |
| `index_text` drops the language and indexes English | **nothing** → `the_language_asked_for_reaches_the_text_index` |
| `index_vectors` drops the kind | `a_newcomer_can_run_every_query_kind` (quantisation cannot train on three rows) |
| `hybrid_search` fuses against an arbitrary vector | **nothing** → `hybrid_search_ranks_the_caller_s_query_and_not_another_vector` |
| `text_search` widens the limit it was given | **nothing** → the limit assertion in `a_newcomer_can_run_every_query_kind` |

Three of the four survivors were a **delegation losing an argument** — the language, the query, the limit.
A facade whose methods forward is exactly where that is invisible, because every other assertion still
looks right: an index gets built, a search answers, hits carry ranks. Each needed a claim that only the
correct argument can satisfy — the German-only stem `-keit`, the query's own nearest neighbour, one result
where two match.

## The gate was verifying against a stale storage layer

`just qa` reported `embed_query` as missing while it was on the screen. `cargo publish --dry-run
--workspace` serves the unpublished members to each other through a throwaway local registry and caches
both the unpacked source and its compiled artefact under name and version — and the version here never
changes, so it reused the first of each forever. `publish-check` now drops both before it runs, which is
why `qa` is trustworthy again rather than needing a `cargo clean` nobody would think of.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 187 tests
cargo +stable clippy --all-targets --all-features -- -D warnings
```

## Acceptance

- [x] AC-72 — a guide that goes from clone to all four query kinds, with the setup and the queries executed
      by `seeded_library` and `a_newcomer_can_run_every_query_kind`, and linked from the README so it can be
      found. It names no criterion id, and it says plainly what is not built yet.
