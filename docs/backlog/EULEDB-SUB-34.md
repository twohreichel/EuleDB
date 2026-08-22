---
id: EULEDB-SUB-34
ticket: EULEDB
fulfils: [AC-2, AC-3, AC-4, AC-5]
depends_on: [EULEDB-SUB-33]
size: L
context_budget: 3000
safety: a benchmark target — no library behaviour changes
detail: stub
status: backlog
---

## Goal

**Publish the KPIs as a benchmark a stranger can run.** Recall@10 >= 0.90 against the brute-force baseline, p95 latency under the two ceilings, resident
memory under 50 MB idle and 200 MB at peak, all four as one reproducible in-repo benchmark run by **one
documented command**, recording the hardware, corpus and commit.

The ceilings are absolute and hardware-independent, which is what makes them portable — and what makes
them fail on the smallest CI runner first.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/backlog/done/EULEDB-SUB-27.md` — the corpus and the baseline
- `docs/specs/spec.md (AC-2, AC-3, AC-4, AC-5)` and § Platform classes
- `justfile`

## Notes for the cut

Measuring resident memory portably is the part that will take the time — the four platforms do
not agree on how to ask. Decide whether the benchmark gates CI or only records: a latency gate on a shared
runner is a flake generator, and a recorded number nobody looks at is decoration. Say which it is.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
