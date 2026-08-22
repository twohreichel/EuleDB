---
id: EULEDB-SUB-28
ticket: EULEDB
fulfils: [AC-32, AC-33]
depends_on: [EULEDB-SUB-27]
size: L
context_budget: 3000
safety: a new module with no caller until the next ticket wires it in
detail: stub
status: backlog
---

## Goal

**Embed text deterministically, 384 dimensions.** Chunk to the model's 512-token limit, apply the E5 prefix convention (`query:` and `passage:`),
L2-normalise, and produce 384-dimensional vectors that are bit-identical across runs on one platform.

**The open decision lands here:** `ort` (bindings to ONNX Runtime) versus `candle-onnx` (pure Rust). It is
consequential for CI, which spans linux-x86_64, linux-aarch64, macos-arm64 and windows — `ort` needs a
native runtime per platform, `candle` does not but supports a narrower slice of ONNX. Measure both against
the model before choosing, and record the number.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/specs/spec.md (AC-32, AC-33)`
- `docs/backlog/done/EULEDB-SUB-27.md`
- `.github/workflows/` — the four platforms the choice has to survive

## Notes for the cut

Bit-identical across runs is the assertion that costs the most to satisfy: thread count and
reduction order both perturb floating point. Pin whatever controls them and say so. The prefix convention
is not decoration — E5 without it loses measurable recall, so a test must show the prefixes reach the
model rather than merely that a vector came back.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
