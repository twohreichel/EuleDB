---
id: EULEDB-SUB-5
ticket: EULEDB
fulfils: [AC-9]
depends_on: [EULEDB-SUB-4]
size: M
context_budget: 3000
safety: CI only
detail: stub
status: backlog
---

## Goal

Workflow hardening. SHA-pin every third-party action, permissions {} plus per-job grants, persist-credentials false, untrusted input only via env, and actionlint plus zizmor enforcing it mechanically.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `.github/workflows/*.yml`
- `docs/specs/spec.md (AC-9)`
- GitHub's own hardening guidance for workflow permissions, SHA-pinning and untrusted input

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
