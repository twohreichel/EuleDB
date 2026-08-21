---
id: EULEDB-SUB-13
ticket: EULEDB
fulfils: [AC-21]
depends_on: [EULEDB-SUB-12]
size: M
context_budget: 3000
safety: additive API
detail: stub
status: backlog
---

## Goal

DEK rotation. Rotate the data-encryption key by re-wrapping it, without rewriting the encrypted payload, keeping previously written data readable.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/*/src/crypto/`
- `docs/specs/spec.md (AC-21)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
