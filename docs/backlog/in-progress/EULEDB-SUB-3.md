---
id: EULEDB-SUB-3
ticket: EULEDB
fulfils: [AC-7, AC-11]
depends_on: [EULEDB-SUB-2]
size: L
context_budget: 3000
safety: CI only, no runtime code
detail: stub
status: backlog
---

## Goal

CI quality pipeline. Separate fmt, clippy, nextest and doc jobs as required status checks, matrixed over linux-x86_64, linux-aarch64, macOS-arm64 and windows-x86_64, against MSRV and stable. A platform not in the matrix must not be claimed as supported (AC-11).

## Context (rough — regenerate this ticket just-in-time before starting it)

- `.github/workflows/ci.yml (new)`
- `justfile`
- `rust-toolchain.toml`
- `docs/specs/spec.md (AC-7, AC-11)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
