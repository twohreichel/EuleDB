---
id: EULEDB-P4
ticket: EULEDB
kind: phase
fulfils: [AC-51, AC-52, AC-53, AC-54, AC-55]
depends_on: [EULEDB-P3]
size: epic
estimate_pm: "2-3"
context_budget: 2000
safety: not a mergeable unit — see below
detail: stub
status: backlog
---

## Goal

**P4 — CRDT sync.** Multi-device convergence without a server: Loro CRDT documents, encrypted deltas, and a transport the caller supplies rather than one EuleDB opens.

## Effort

**2-3 person-months** for one experienced developer, per the research estimate (concept § 5). The
total across P0-P5 is 17-21. The number assumes the chosen crates hold up and the UX scope does not
grow — treat a large overrun as a signal to re-cut, not to work longer.

## This is a phase, not a ticket

`size: epic
estimate_pm: "2-3"` is deliberate and it is a warning, not a label. This exceeds `L`, so it is **not
executable as one session and must not be started as one.** When P4 becomes next, cut it into
`EULEDB-SUB-<n>` tickets of size S to L before any work starts, then work those one at a
time. This file exists so the criteria below are visible in the backlog instead of being remembered.

## Criteria in scope

- Loro documents converging regardless of delta order (AC-51)
- concurrent row modification converging, resolution recorded in the audit log (AC-52)
- deltas encrypted under the AES-256-GCM scheme of AC-20, rejected on a failed tag (AC-53)
- transport-agnostic: no socket opened by the library (AC-54)
- reconnect exchanges deltas only, never a full document (AC-55)

## Notes for the cut

The audit-log dependency in AC-52 means P1 must be done, not merely started.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this phase is cut. A ticket
detailed today against a repository state an earlier phase will change is wrong by the time it is
picked up — which is exactly why this file stays coarse.
