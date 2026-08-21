---
id: EULEDB-SUB-11
ticket: EULEDB
fulfils: [AC-18, AC-19]
depends_on: [EULEDB-SUB-10]
size: M
context_budget: 3000
safety: per-table option, default unchanged
detail: stub
status: backlog
---

## Goal

Block and string compression. zstd block compression with a per-table level. For strings: MEASURE what Lance already does with FSST and dictionary encoding, and write no own encoder before that measurement exists.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/*/src/storage/`
- `docs/specs/spec.md (AC-18, AC-19)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
