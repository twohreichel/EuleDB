---
id: EULEDB-SUB-9
ticket: EULEDB
fulfils: [AC-15]
depends_on: [EULEDB-SUB-7]
size: M
context_budget: 3000
safety: new crate, no consumer yet
detail: stub
status: backlog
---

## Goal

Arrow schema and insert validation. Define a table schema as an Arrow schema and reject a mismatching record batch on insert, naming the offending column and the mismatch.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/*/src/schema.rs (new)`
- `docs/specs/spec.md (AC-15)`
- `docs/specs/spec.md § Glossary`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
