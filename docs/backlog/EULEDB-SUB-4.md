---
id: EULEDB-SUB-4
ticket: EULEDB
fulfils: [AC-8]
depends_on: [EULEDB-SUB-3]
size: M
context_budget: 3000
safety: CI only
detail: stub
status: backlog
---

## Goal

Supply-chain gates. cargo-deny (advisories, licences, bans, sources) and cargo-audit on every PR and on a weekly schedule, with the licence check gated to Apache-2.0 OR MIT.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `deny.toml (new)`
- `.github/workflows/security.yml (new)`
- `Cargo.toml`
- `docs/specs/spec.md (AC-8)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
