---
id: EULEDB-SUB-28
ticket: EULEDB
fulfils: [AC-32, AC-33]
depends_on: [EULEDB-SUB-27]
size: L
context_budget: 3000
safety: a new crate with no caller until the next ticket wires it in
detail: full
status: done
---

## Goal

**Embed text deterministically, 384 dimensions.** Chunk to the model's 512-token window, apply the E5
prefix convention, L2-normalise, and produce vectors that are bit-identical across runs on one platform.

## The open decision, closed by evidence rather than argument

`ort` against `candle-onnx` was framed as op coverage versus build complexity. **What decided it is supply
chain.** `ort`'s default features download a prebuilt C++ ONNX Runtime at build time over TLS, and that
artefact sits outside every gate this project has: `cargo-deny` inspects Rust crates, and CI pins
third-party actions to commit SHAs precisely so no unverified mutable artefact enters a build. Using `ort`
without that feature instead demands a native runtime installed on every developer machine and all four CI
platforms — heavier than the one build tool already required. `ort` 2.0 is also still a release candidate.

The op-coverage worry was then settled by running it: `candle-onnx` loads this exact graph and returns
`last_hidden_state` of shape `[1, n, 384]`. It needs `protoc`, which the on-disk format already requires.

## Two costs, both recorded

**The dependency tree grows by about 195 crates** against `ort`'s 51. Seven duplicate-version entries were
added to `deny.toml` with reasons — and one, `tokenizers`, was **removed rather than skipped** by aligning
the declaration to the version `candle-core` already uses. Two copies of a 250 000-entry vocabulary in one
binary for nothing.

**The MSRV rises from 1.91.0 to 1.94.0.** `candle-core` declares no `rust-version` while using an unstable
feature on aarch64, so its requirement was **measured**: 1.93.0 fails, 1.94.0 builds. The MSRV was always
derived from what dependencies need rather than chosen, so this is the same rule applied to a dependency
that does not declare its own — and `rust-toolchain.toml` says so.

## What the mutation pass found, and both findings were real

Five mutations, three caught at once.

**Masked pooling was unreachable.** Ignoring the attention mask changed nothing, because one text is
encoded at a time and nothing is padded — the mask branch was a branch no caller could reach. Removed
rather than defended, with a note that it returns together with batching and a test that pads.

**Pooling the leading token instead of the mean passed every test.** 384 dimensions, unit length,
prefix-sensitivity, determinism — all hold for a vector pooled the wrong way. E5 is trained with mean
pooling, so that mutation produces embeddings that are stable, cheap and *not the model's*, which is
precisely the failure that running the exported graph exists to avoid. Only retrieval can tell: a query
must sit measurably closer to the passage answering it than to an unrelated one. That test now exists, with
a margin rather than a bare ordering, and it catches the mutation.

## A test that had to be moved rather than kept

Embedding a 20 000-character document is one forward pass per chunk through a twelve-layer transformer, and
in a debug build that is minutes — the first version of the chunking test hung. Chunking is arithmetic over
tokenisation, so it is tested where it lives: `chunks` and `token_count` are public for that reason, and
asserting a token budget no longer costs a forward pass. The suite runs in 3.3 seconds in debug.

## Verification

```bash
just model                                         # fetches 470 MB at a pinned revision, once
just format && just lint && just test && just qa   # all green, 151 tests
```

| Mutation | Caught by |
|---|---|
| the E5 prefixes are dropped | `the_query_and_passage_prefixes_reach_the_model` |
| vectors are not L2-normalised | `every_vector_is_l2_normalised` |
| the chunk budget ignores the prefix and the special tokens | `text_beyond_the_token_limit_becomes_several_chunks_that_each_fit` |
| the attention mask is ignored | moot — the branch was unreachable and was removed |
| the leading token is used instead of the mean | `a_query_is_closer_to_its_answer_than_to_an_unrelated_passage`, added for it |

## Acceptance

- [x] AC-33 — 384 dimensions, from the model's own exported graph, bit-identical across runs.
- [x] AC-32 — chunked to the token window with the prefix and the special tokens counted against it,
      E5 prefixes applied and shown to reach the model, every vector L2-normalised.
- [x] The model is fetched at a pinned revision, digests printed, and CI caches it by that revision.
- [x] An absent model names the command that fetches it.
