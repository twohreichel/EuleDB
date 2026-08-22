---
id: EULEDB-SUB-15
ticket: EULEDB
fulfils: [AC-67, AC-68]
depends_on: [EULEDB-SUB-13]
size: L
context_budget: 3000
safety: additive API — existing insert and scan paths unchanged
detail: stub
status: backlog
---

## Goal

Row mutations: update and delete. Update rows matching a predicate and delete rows matching a predicate. Both were presupposed by other criteria (auto-embedding runs on update, the capability model gates delete, the NL path refuses destructive operations) but never specified. Delete MUST log the affected count and the scoping predicate BEFORE executing.

## Why this exists at all

Missing from the original concept, and therefore missing from the first draft of the spec. Found by
asking a different question than "is the concept covered": **what does a database need that the concept
never mentioned?** Without these there is no working database, so they are P0, not polish.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/ (mutation path)`
- `docs/specs/spec.md (AC-67, AC-68)`
- `docs/specs/spec.md (AC-6 capability scope, AC-29 audit log)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
