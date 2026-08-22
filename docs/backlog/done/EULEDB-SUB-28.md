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

## The open decision took three candidates and two reversals

The question was framed as op coverage against build complexity. Neither decided it.

**`ort` was rejected on supply chain.** Its default features download a prebuilt C++ ONNX Runtime at build
time over TLS, outside every gate this project has: `cargo-deny` inspects Rust crates, and CI pins
third-party actions to commit SHAs precisely so no unverified mutable artefact enters a build. Without that
feature it instead needs a native runtime on every developer machine and all four CI platforms. `ort` 2.0
is also still a release candidate.

**`candle-onnx` was chosen, implemented, and disproven by CI.** It ran the graph correctly here, and it
does not build on **linux-aarch64** at all: its `gemm` dependency emits aarch64 assembly requiring the
`fullfp16` CPU feature, which is not in that target's default baseline. `candle-core` declares `gemm` with
default features and Cargo features are additive, so the f16 path cannot be disabled from downstream.
Enabling `+fp16` for that target would make the binary require ARMv8.2-FP16 at runtime — against criteria
that are explicitly hardware-independent. It also needed a raised MSRV: it declares none while requiring
1.94.0, measured, 1.93.0 fails.

**`tract-onnx` has neither problem.** Pure Rust, no build tool, no CPU-feature assembly, and it declares
`rust-version = 1.91` — the MSRV this workspace already had, so the raise was reverted. It runs the graph
and returns `[1, n, 384]`.

This is the second time the four-platform matrix has disproven a decision that looked settled, and both
times the alternative was only visible after the failure. Worth naming as a pattern rather than a run of
bad luck.

## The cost of tract, measured

`tract` compiles a graph for a **fixed** input shape — the attention layers reshape in a way it cannot
resolve symbolically. So the sequence length is decided at compile time, and the options were measured
rather than guessed:

| Approach | Cost |
|---|---|
| pad everything to the 512-token window | **108 ms per call** — the whole AC-3 latency budget on one embedding |
| compile per exact token count | ~130 ms per plan, one plan per distinct length |
| **six buckets, pad to the next** | ~4 ms for a short query, ~108 ms for a full chunk, waste under 2x, six plans |

Buckets it is. And the padding makes the attention mask load-bearing again — the earlier design had none,
and the masked-pooling branch was removed as unreachable. It is back, with the padding that gives it
meaning.

One MPL-2.0 exception joined `deny.toml` (`dyn-eq`, transitive under `tract-core`) beside the existing one,
with the same reasoning: transitive, file-level copyleft, no constraint on the larger work.

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
| the attention mask is ignored | **survived, and is recorded as a gap.** Measured on a representative pair: cosine 0.8366 unmasked against 0.8165 masked. Real, and far too small for an honest threshold — tightening the bound to 0.83 would be a number reverse-engineered from the mutation. The mask stays because pooling padding is wrong, not because a test catches it; testing it properly needs one text embedded at two bucket sizes, which means an API existing only for the test |
| the leading token is used instead of the mean | `a_query_is_closer_to_its_answer_than_to_an_unrelated_passage`, added for it |

## Acceptance

- [x] AC-33 — 384 dimensions, from the model's own exported graph, bit-identical across runs.
- [x] AC-32 — chunked to the token window with the prefix and the special tokens counted against it,
      E5 prefixes applied and shown to reach the model, every vector L2-normalised.
- [x] The model is fetched at a pinned revision, digests printed, and CI caches it by that revision.
- [x] An absent model names the command that fetches it.
