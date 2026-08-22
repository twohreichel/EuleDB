---
id: EULEDB-SUB-35
ticket: EULEDB
fulfils: [AC-64]
depends_on: [EULEDB-SUB-34]
size: S
context_budget: 3000
safety: documentation only
detail: stub
status: backlog
---

## Goal

**Publish the community metrics beside the technical ones.** Stars, contributor count, downloads per month, and time to the first externally authored merged
pull request — recorded with the date they were taken, beside the KPIs of AC-5.

They gate the phase-3 decision, so they are evidence rather than decoration. Two of them are zero at the
first recording, and writing a zero down is the point.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/specs/spec.md (AC-64)` and § Decisions taken
- `docs/backlog/done/EULEDB-SUB-34.md`

## Notes for the cut

No live API call from the build: a metric that changes when the benchmark runs is not a
recording. Take them by hand, date them, commit them.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
