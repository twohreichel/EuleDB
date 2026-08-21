---
id: EULEDB-P1
ticket: EULEDB
kind: phase
fulfils: [AC-6, AC-24, AC-25, AC-26, AC-27, AC-28, AC-29, AC-30]
depends_on: [EULEDB-SUB-14]
size: epic
estimate_pm: "2-3"
context_budget: 2000
safety: not a mergeable unit — see below
detail: stub
status: backlog
---

## Goal

**P1 — Indices and exact queries.** Exact lookups and predicate evaluation get their own indices, and access control plus the audit log arrive — which is where the read-only default of AC-6 becomes real rather than declared.

## Effort

**2-3 person-months** for one experienced developer, per the research estimate (concept § 5). The
total across P0-P5 is 17-21. The number assumes the chosen crates hold up and the UX scope does not
grow — treat a large overrun as a signal to re-cut, not to work longer.

## This is a phase, not a ticket

`size: epic
estimate_pm: "2-3"` is deliberate and it is a warning, not a label. This exceeds `L`, so it is **not
executable as one session and must not be started as one.** When P1 becomes next, cut it into
`EULEDB-SUB-<n>` tickets of size S to L before any work starts, then work those one at a
time. This file exists so the criteria below are visible in the backlog instead of being remembered.

## Criteria in scope

- ART point and range index on key columns (AC-24, AC-25)
- Roaring bitmap predicate evaluation against a brute-force reference (AC-26)
- exact filter applied as a pre-filter before candidate generation (AC-27)
- signed capability tokens, read-only by default (AC-6, AC-28)
- hash-chained append-only audit log, with chain verification (AC-29, AC-30)

## Notes for the cut

Decide between `art-tree`, `art-rs` and a SIMD variant — the spec deliberately leaves this open (§ Technology stack, status `evaluate`).

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this phase is cut. A ticket
detailed today against a repository state an earlier phase will change is wrong by the time it is
picked up — which is exactly why this file stays coarse.
