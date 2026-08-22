---
id: EULEDB-SUB-33
ticket: EULEDB
fulfils: [AC-37, AC-38, AC-39]
depends_on: [EULEDB-SUB-30, EULEDB-SUB-32]
size: L
context_budget: 3000
safety: fusion is a new call over two existing sources — neither changes
detail: full
status: done
---

## Goal

**Fuse the two candidate lists, and say where each hit came from.** Reciprocal Rank Fusion,
`score(d) = sum 1/(k + rank)`, `k = 60` by default, a smaller `k` below a hundred documents with the
effective value reported, and per-source ranks on every hit.

## What landed

`Fused`, `FusedHit`, the fusion itself, and `LanceStore::hybrid_search`. Every hit carries the rank it held
on each side, because a fused score without that is an unexplainable number — and an unexplainable ranking
is one nobody can debug or trust.

## The expected values are the formula, computed by hand

Six unit tests over hand-built rank lists, with the arithmetic written at each assertion: `1/16 + 1/17 =
0.121324` for a row both sides found, `1/16 = 0.0625` for one only the lexical side found. Nothing is
derived from the implementation.

The assertion the ticket asked for is there and it is the one that matters: **a row both sides ranked
third outranks a row a single side ranked first** — `1/18 + 1/18 = 0.111111` against `1/16 = 0.0625`. A
fusion that merely concatenated would put the second one first.

Two more that were not in the notes and earned their place through the mutation pass: ranks count **from
one**, because counting from zero divides by `k` alone at the top and changes every score; and ties break
by row id, because a ranking that reorders equal scores between runs cannot be paginated.

## Why `k` is chosen from the table and not from the lists

Two sources may each return ten candidates out of a million rows, and it is the million that decides
whether adjacent ranks need separating. With `k = 60` the scores of rank 1 and rank 20 differ by a few
percent — on a corpus of a few dozen documents that is no ordering at all, which is why a smaller `k`
exists and why the value used is reported rather than assumed.

## One mutation survived, and it is the fourth of its kind in this phase

Each side is asked for twice the caller's limit, so fusion has something to reorder rather than merging two
already-truncated lists. **Asking for exactly the limit survives every test here.** The effect of that
breadth is a recall difference, and a recall difference over two dozen documents is not measurable — the
same shape as the HNSW ranking tail, the quantised index's size direction, and the agreement between the
two index kinds. At test scale the interface claim is available and the quality claim is not; all four are
recorded rather than papered over with a threshold that would hold only where it was written.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 182 tests
```

| Mutation | Caught by |
|---|---|
| the small-corpus `k` is never used | `the_scores_are_the_formula`, `a_small_corpus_reports_the_k_it_actually_used` |
| the second source replaces rather than adds | `a_row_both_sources_found_beats_one_either_found_alone` |
| ranks count from zero | `the_scores_are_the_formula` |
| ties keep their arrival order | `equal_scores_break_by_row_so_the_order_is_stable` |
| the limit does not truncate | `the_limit_truncates_the_fused_ranking` |
| the lexical side is never consulted | `a_row_both_paths_found_outranks_one_only_one_path_found` |
| each side is asked for exactly the limit | nothing — see above |

## Acceptance

- [x] AC-37 — the formula, with `k = 60` above the threshold, asserted against hand-computed values.
- [x] AC-38 — a smaller `k` below a hundred documents, and the effective value reported on the result.
- [x] AC-39 — per-source ranks on every hit, so a caller sees whether a hit came from the semantic side,
      the lexical side or both.
