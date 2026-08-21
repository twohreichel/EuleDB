---
id: EULEDB-SUB-16
ticket: EULEDB
fulfils: [AC-69, AC-70]
depends_on: [EULEDB-SUB-15]
size: L
context_budget: 3000
safety: hardens existing paths — no new public surface
detail: stub
status: backlog
---

## Goal

Crash safety and the concurrency model. An interrupted write must leave the database in the state before or after it, never in between — proven by killing the writer at multiple points, not by argument. And the reader/writer model must be decided, enforced and documented: multiple readers, at most one writer, a second writer gets a clear error instead of corruption or an indefinite block.

## Why this exists at all

Missing from the original concept, and therefore missing from the first draft of the spec. Found by
asking a different question than "is the concept covered": **what does a database need that the concept
never mentioned?** Without these there is no working database, so they are P0, not polish.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/`
- `docs/specs/spec.md (AC-69, AC-70)`
- `docs/adr/ADR-001-lance-as-storage-format.md (what Lance versioning already guarantees)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
