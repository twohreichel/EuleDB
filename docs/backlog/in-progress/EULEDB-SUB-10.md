---
id: EULEDB-SUB-10
ticket: EULEDB
fulfils: [AC-16, AC-17]
depends_on: [EULEDB-SUB-9]
size: L
context_budget: 3000
safety: trait boundary keeps Lance swappable
detail: stub
status: backlog
---

## Goal

Lance persistence behind storage trait. Persist to Lance and return byte-identical rows after reopen, reached only through an internal storage trait, with Lance pinned to an exact version. Read ADR-001 first.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/*/src/storage/ (new)`
- `docs/adr/ADR-001-lance-as-storage-format.md`
- `docs/specs/spec.md (AC-16, AC-17)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
