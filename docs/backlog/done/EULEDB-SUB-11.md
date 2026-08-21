---
id: EULEDB-SUB-11
ticket: EULEDB
fulfils: [AC-18, AC-19]
depends_on: [EULEDB-SUB-10]
size: M
context_budget: 3000
safety: per-table option, existing tables unaffected
detail: full
status: done
pr: https://github.com/twohreichel/EuleDB/pull/11
---

## Goal

zstd block compression with a level configurable per table at creation time. For strings: **measure what
the format already does** with FSST and dictionary encoding, and write no own encoder before that
measurement exists.

## Context (read ONLY these files)

- `crates/euledb-storage/src/compression.rs`, `src/definition.rs`, `src/store.rs`
- `crates/euledb-storage/tests/compression.rs`
- `crates/euledb-storage/examples/measure_encoding.rs`
- `crates/euledb-storage/README.md`
- `docs/specs/spec.md` (AC-18, AC-19)

## The measurement came first, and it changed the design three times

The format configures encoding through **Arrow field metadata** — `lance-encoding:compression` and
`lance-encoding:compression-level` — so every option is reachable without touching its internals.
20 000 rows of repetitive multilingual legal prose, 2.53 MB of raw text, three runs per configuration,
data files only because the manifest varies:

| Configuration | Data bytes | Stable across runs |
|---|---:|---|
| `compression = none` on the text columns | 2 749 453 | yes |
| nothing declared, the format chooses | 681 622 | **no — varied by 97 KB** |
| FSST forced on the text columns | 886 244 | **no — varied by 44 KB** |
| **zstd level 1 on every column** | **649 029** | **yes** |
| zstd level 9 on every column | 647 813 | yes |
| zstd level 22 neighbourhood | 637 640 | yes |
| zstd level 1 on the text columns only | 673 518 | yes |

Four findings, three of them contrary to what I would have written without measuring:

1. **The automatic choice is not reproducible.** Identical input, 20 % swing in on-disk size. Explicit
   zstd is byte-identical across runs. That is the deciding argument, ahead of the 5 % size win: a
   stored size that moves on its own cannot be compared against a later one, which makes every figure a
   benchmark records meaningless.
2. **Level 1, not 9 or 22.** Under 2 % smaller at the top of the range for several times the
   compression work, on platforms that already run inference on four cores. And **size is not monotonic
   in the level**: level 3, zstd's own default, measured *larger* than level 1.
3. **Declare it on every column, not only the text ones.** Everywhere was 4 % smaller than strings-only,
   which was the opposite of the expectation.
4. **No own string encoder, and AC-19 is answered by evidence.** Forcing FSST by hand was *worse* than
   letting the format decide, so it is doing something better than plain FSST. Writing an encoder to
   compete with that would spend the project's scarcest resource on the one layer where it has no
   advantage. Stated in `crates/euledb-storage/README.md`, which is the crate's page on the registry.

## Design

`Compression` is an enum of `Zstd(ZstdLevel)` and `None`, defaulting to zstd at `ZstdLevel::DEFAULT`.
`ZstdLevel` is a **newtype with a validating constructor**, because the range 1 to 22 is not obvious and
an out-of-range value would otherwise fail at write time, far from where it was set.

`TableDefinition` bundles the schema and the compression. A parameter bundle rather than a widening
signature: the settings a table is created with only grow, each is fixed for the table's life, and this
is where the single configuration mechanism AC-74 asks for will live.

The compression travels as field metadata on the schema, so it is persisted **with** the table rather
than having to be supplied again on every write.

## The leak the tests caught

Attaching the metadata to the schema made it come back on every scan, so a caller who wrote a batch and
read it back got a schema decorated with this crate's storage configuration — and the two existing
"rows come back unchanged" tests failed on exactly that. Fixed at the source rather than by relaxing the
assertions: `scan` strips the encoding keys, so the caller gets *their* schema back.

## TDD record, and two gaps the mutation check found

Four tests written first, all RED on `no Compression in the root`. Then mutations:

| Mutation | Caught | Response |
|---|---|---|
| level hardcoded to 1 | yes | — |
| `scan` stops stripping the encoding keys | yes, 2 tests | — |
| **`Compression::None` emits no metadata** | **no** | the format then compresses on its own, so "smaller than uncompressed" still held. The assertion now demands a **factor of 2**, which separates real absence of compression (4.2x) from the automatic choice (1.15x) |
| **a named level constant drifts out of range** | **no** | the constants bypass the validating constructor, so a test now asserts each one is a level the constructor would accept |
| the default level changed | no | **deliberate.** Pinning the default in a test would be a tautology — it is a choice, documented with its measurement, not a behaviour |

A test for rejecting levels outside 1 to 22 was added at the same time: the newtype's only purpose is
that rejection, and it was untested.

## Test size, chosen by measurement

The suite uses 2 000 rows, not 20 000. At 2 000 uncompressed is still 4.2 times compressed and two zstd
levels still differ by over a thousand bytes, so every assertion keeps a comfortable margin — and the
four tests run in **0.38 s instead of 27**.

The measurement harness is an **example**, not a test: it reports and asserts nothing, and a test that
asserts nothing is noise in a suite. `cargo run --release --example measure_encoding -p euledb-storage`.

## Verification (executable)

```bash
just format && just lint && just test && just qa    # protoc must be on PATH

# the measurement anyone can repeat, including the reviewer
cargo run --release --example measure_encoding -p euledb-storage

# every claim in the compression path is guarded — break it, confirm a test notices
#   level hardcoded                     -> 1 test fails
#   Compression::None emits no metadata -> 1 test fails
#   scan stops stripping the keys       -> 2 tests fail
#   a named constant leaves the range   -> 1 test fails
```

## Out of scope / Guardrails

- **No encryption** — SUB-12. Compression must happen before encryption, and the ordering is that
  ticket's problem, not this one's.
- **No own string encoder, now or later**, without a measurement that beats the format. That is what
  AC-19 asks for and the current numbers say do not bother.
- **No compression change on an existing table.** It would mean rewriting the data, so the setting is
  deliberately creation-time only.
- Do not raise the default level on the strength of the table above. Two per cent is not worth several
  times the CPU on a four-core machine that is also running inference.

## The gate went red on aarch64, and why

Both linux-aarch64 legs were killed with **SIGTERM during the build** — exit 143, at seven and a half
minutes, well inside the thirty-minute timeout — while x86_64, macOS and Windows all passed. Two
independent runners, two different times, so not a cancellation and not a flake. The aarch64 images have
less disk and less memory than the x86_64 ones, and the dependency tree the previous ticket introduced is
477 crates including datafusion.

Three changes, in order of how much they are guesses:

1. **`debug = "line-tables-only"` for the dev and test profiles.** Measured, not assumed: `target/` went
   from **7.7 GB to 4.7 GB**. Line tables still give file and line in a backtrace, which is what a test
   failure needs — full DWARF is only needed for a step debugger, and that can be turned back on locally.
2. **Dependency caching**, keyed per platform and toolchain. Deferred in SUB-3 with the reason "it would
   save nothing on a 16-package tree, revisit when Lance lands". It has landed. Beyond time, it lowers
   the peak: a cached build compiles a handful of crates instead of hundreds.
3. **A step reporting free disk and memory before the build.** The first failure cost two rounds of
   speculation because SIGTERM says nothing about which resource ran out. The next one will say.

## Definition of Done

- [x] AC-18 covered: zstd applied, level configurable per table at creation, and both facts tested
- [x] AC-19 covered: the format's string encoding measured, the finding documented on the crate's
      registry page, and no own encoder written
- [x] Every test observed failing first, and every gap the mutation check found closed
- [x] The measurement is reproducible by one documented command
- [x] Rows still come back unchanged, with the storage configuration stripped from the caller's schema
- [x] Commits follow Conventional Commits, grouped by concern
- [x] The pipeline is green on all four platforms, and the aarch64 failure diagnosed rather than retried
