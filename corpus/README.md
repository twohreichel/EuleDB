# Reference corpus

Every KPI this project publishes is measured against the corpus described here. It is recorded in this
file rather than in a commit message because a third party has to be able to reproduce the numbers, and
a corpus nobody can identify makes every recorded figure unfalsifiable.

## Provenance

| | |
|---|---|
| Source | [`wikimedia/wikipedia`](https://huggingface.co/datasets/wikimedia/wikipedia) on Hugging Face |
| Snapshot | `20231101` — a dated snapshot, never "latest" |
| Languages | `de`, `fr`, `pl`, `en` |
| Window | 500 rows per language from offset 1000, filtered to documents of at least 500 characters |
| Documents | 1 905 (the filter removes the short stubs, deterministically) |
| Size | 35.6 MB |
| SHA-256 | `f85e3748907f2a7b9873e317d3d325d1ab8757e521d473f41c3cc618ce14b196` |

Three of the four languages are morphologically different from each other, which matters: the embedding
model is multilingual, and a single-language corpus would measure the easy half of what it claims.

The offset skips the alphabetically first articles, which are unusually short stubs, and the length filter
removes the rest of them. A stub carries no signal for retrieval and would flatter every recall number.

## Licence and attribution

Wikipedia text is licensed **CC BY-SA 4.0**. The corpus therefore carries that licence, not this
repository's `Apache-2.0 OR MIT` — the code and the corpus are separately licensed, which is why the
corpus lives in its own directory with this file beside it.

Attribution: the text is authored by Wikipedia contributors. Each document keeps its article title, and
its `id` is the Wikipedia page id prefixed with the language code, so any document can be traced back to
its article and its revision history.

## The two corpora, and why there are two

**`smoke.tsv` is tracked** — 39 documents, four languages, 486 KB. Small enough to live in the repository,
so the test suite needs no network and no fetch step. Too small for a benchmark: recall over 39 documents
says nothing.

**`reference.tsv` is not tracked** and is fetched on demand:

```sh
just corpus
```

Thirty-five megabytes of somebody else's prose does not belong in a source repository. The digest above is
pinned in code (`euledb_corpus::REFERENCE_DIGEST`) and checked on load, so a corpus that has drifted is
refused rather than silently measured — measuring against a different corpus than the recorded numbers
came from is worse than not measuring.

## Reproducing it

```sh
python3 scripts/fetch-corpus.py           # writes corpus/reference.tsv and prints its digest
python3 scripts/fetch-corpus.py --smoke   # regenerates the tracked subset
```

The fetcher takes the same window every time, so the same command yields the same documents on any
machine. If the digest it prints differs from the one above, the upstream snapshot changed and the recorded
numbers belong to the older one — say so rather than updating the digest quietly.
