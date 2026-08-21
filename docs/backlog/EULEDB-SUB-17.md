---
id: EULEDB-SUB-17
ticket: EULEDB
fulfils: [AC-71]
depends_on: [EULEDB-SUB-16]
size: M
context_budget: 3000
safety: defines the surface every later ticket returns through
detail: stub
status: backlog
---

## Goal

Public error type. One documented error type for every failure. The public API must not panic on malformed input, a missing or unreadable file, a permission error or a failed decryption — a library that aborts its host process on bad data cannot be embedded.

## Why this exists at all

Missing from the original concept, and therefore missing from the first draft of the spec. Found by
asking a different question than "is the concept covered": **what does a database need that the concept
never mentioned?** Without these there is no working database, so they are P0, not polish.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb/src/error.rs (new)`
- `docs/specs/spec.md (AC-71)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
