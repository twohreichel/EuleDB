---
id: EULEDB-SUB-7
ticket: EULEDB
fulfils: [AC-12, AC-13]
depends_on: [EULEDB-SUB-6]
size: M
context_budget: 3000
safety: release PR is inert until merged
detail: stub
status: backlog
---

## Goal

Release Please versioning and changelog. Wire the release-please workflow so version and CHANGELOG.md are derived from Conventional Commits, and publishing happens only from a tag created by a merged release PR after the gates pass.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `release-please-config.json`
- `.release-please-manifest.json`
- `.github/workflows/release-please.yml (new)`
- `Cargo.toml` (publish metadata from AC-65 must already be green under `cargo publish --dry-run`)
- `docs/specs/spec.md (AC-12, AC-13)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
