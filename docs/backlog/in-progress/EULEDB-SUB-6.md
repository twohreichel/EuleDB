---
id: EULEDB-SUB-6
ticket: EULEDB
fulfils: [AC-10]
depends_on: [EULEDB-SUB-5]
size: S
context_budget: 3000
safety: config only
detail: stub
status: backlog
---

## Goal

Dependabot configuration. Dependabot for cargo and github-actions, weekly, patch and minor grouped, majors ungrouped, cooldown set, no blanket ignore, no auto-merge for actions.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `.github/dependabot.yml (new)`
- `docs/specs/spec.md (AC-10)`
- GitHub's own hardening guidance for workflow permissions, SHA-pinning and untrusted input

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
