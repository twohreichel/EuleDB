---
id: EULEDB-SUB-27
ticket: EULEDB
fulfils: []
depends_on: [EULEDB-SUB-26]
size: M
context_budget: 3000
safety: test fixtures and a documented corpus — no production path changes
detail: full
status: done
---

## Goal

**Fix the reference corpus before any number is recorded.** Fulfils no criterion and gates four: AC-2
compares recall against an exhaustive baseline **over the same corpus**, and AC-5 requires a third party to
reproduce the numbers with one documented command. A corpus fixed after the first benchmark makes both
unverifiable.

## What landed

`corpus/README.md` recording provenance, licence and attribution, `corpus/smoke.tsv` (39 documents,
tracked), `scripts/fetch-corpus.py`, `just corpus`, and the `euledb-corpus` crate that loads either.

**The corpus:** a fixed window of the dated `20231101` Wikipedia snapshot — 500 rows per language from
offset 1000 in `de`, `fr`, `pl` and `en`, filtered to documents of at least 500 characters. 1 905
documents, 35.6 MB, digest pinned in code and checked on load.

Three of the four languages are morphologically different from each other. The embedding model is
multilingual, and a single-language corpus would measure the easy half of what it claims.

## Two corpora, and why

**Tracked:** 39 documents, 486 KB, embedded with `include_str!` so the test suite needs no network and no
fetch step. Too small for a benchmark — recall over 39 documents says nothing.

**Not tracked:** the 35.6 MB reference corpus. Somebody else's prose at that size does not belong in a
source repository, and the licence differs from the code's: Wikipedia text is CC BY-SA 4.0, this
repository is `Apache-2.0 OR MIT`. The corpus lives in its own directory with its licence beside it, and
each document keeps its title and a language-prefixed page id so it can be traced to its article.

A drifted corpus is **refused, not measured**. Measuring against a different corpus than the recorded
numbers came from is worse than not measuring, and the failure names `just corpus` — a benchmark that
fails with "no such file" and no instruction is a benchmark nobody runs.

## What this ticket does not do

The **brute-force baseline** AC-2 compares against needs embeddings, which arrive in SUB-28. The ticket's
own notes said "with a brute-force baseline computed over it", and that half is not here: there is nothing
to embed with yet. It belongs to SUB-30, where the recall assertion lives. Said plainly rather than left
as a half-kept promise.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 144 tests
just corpus                                        # fetches and prints the digest
```

Four mutations applied by hand, three caught at once and one after a test was widened:

| Mutation | Caught by |
|---|---|
| the pinned digest is not checked | `a_corpus_that_is_not_the_pinned_one_is_refused` |
| escaped separators are left escaped | `the_line_format_survives_text_that_contains_its_separators` |
| the missing-corpus message drops the command that fixes it | `an_unfetched_corpus_says_how_to_fetch_it` |
| **a line with extra fields is accepted** | `a_malformed_line_is_refused_rather_than_guessed_at`, after it was widened — it checked only lines with too *few* fields |

## Acceptance

- [x] The corpus is documented with provenance, snapshot, languages, counts, size, digest and licence.
- [x] One command fetches it, and the same command yields the same documents on any machine.
- [x] A corpus that is not the pinned one is refused, naming both digests.
- [x] The tracked subset needs no network, and its shape is asserted against what the README claims.
