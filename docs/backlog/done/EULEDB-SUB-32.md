---
id: EULEDB-SUB-32
ticket: EULEDB
fulfils: [AC-36]
depends_on: [EULEDB-SUB-27]
size: L
context_budget: 3000
safety: a separate index and a new query path — the vector side is untouched
detail: full
status: done
---

## Goal

**BM25 full text with stable ranking.** And, first, the decision the cut deferred here: whether to add
Tantivy as a second engine or use the index the format already ships.

## The decision, settled by reading rather than by preference

**The format's inverted index, not a second engine.** The stack table named Tantivy and gave stemming
across seventeen Latin languages as the reason. That reason is already met: the format's inverted index
stems through `rust-stemmers` — the same Snowball library Tantivy uses — in **eighteen languages**, and its
tokenizer pipeline is built on the same filters. Adding Tantivy would put a second full-text engine in the
tree, with its own index files, its own tokenisation and its own consistency problem against row identity,
to obtain what is already present.

**Polish is absent from that list, and would be absent from Tantivy's too** — Snowball has no Polish
stemmer. A Polish column is indexed without stemming whichever engine is chosen. The reference corpus
contains Polish, so this is worth knowing rather than discovering.

## What landed

`StemmingLanguage`, `create_text_index`, `search_text`. One language per index, because a Snowball stemmer
is language-specific: it strips German endings from German words and would produce nonsense on French.

## Two test premises of mine were wrong, and measurement fixed both

**`speichern` does not relate to `Speicherung`.** Snowball's German stemmer does not strip `-ung`. It does
strip `-es` and `-en`, so `Wasserstandes` and `Wasserstand` share a stem — that pair works and is what the
test uses now. The premise was wrong, not the index.

**And that pair does not show the German stemmer was used.** English strips `-es` too, so a mutation
forcing English survived it. The discriminating pair, also measured: `verhältnismäßig` finds
`Verhältnismäßigkeit` under German — which strips `-keit` — and finds **nothing** under English. That is
the test that defends the language.

## A false finding, caught before it was reported

One mutation appeared to survive. It had not been applied: `just format` had reflowed the call, so the
search string written before formatting no longer matched and the edit silently did nothing. Applied
properly, it is caught. **A mutation pass has to verify that the file actually changed** — otherwise a
failed edit reads as a surviving mutation, which is a fabricated finding.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 173 tests
```

| Mutation | Caught by |
|---|---|
| stemming is switched off | `stemming_relates_the_inflected_forms_it_actually_relates` |
| the language is ignored, always English | `the_language_reaches_the_index`, added for it |
| the limit is ignored | `the_limit_bounds_the_result`, added for it |
| the query text is replaced by a constant | `a_full_text_query_ranks_the_matching_documents` |

## Acceptance

- [x] AC-36 — a BM25 query returns the matching rows, and identical runs rank identically.
- [x] AC-36 — stemming reaches the index, shown by a pair that only the chosen language relates.
- [x] The engine decision is recorded with the evidence, including what neither engine can do.
