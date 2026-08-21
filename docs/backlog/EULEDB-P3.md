---
id: EULEDB-P3
ticket: EULEDB
kind: phase
fulfils: [AC-40, AC-41, AC-42, AC-43, AC-44, AC-45, AC-46, AC-47, AC-48, AC-49, AC-50]
depends_on: [EULEDB-P2]
size: epic
estimate_pm: "3-4"
context_budget: 2000
safety: not a mergeable unit — see below
detail: stub
status: backlog
---

## Goal

**P3 — Natural language to intermediate representation.** The sandboxed natural-language layer — the actual differentiator. A local model emits a typed IR, never SQL and never code, and a deterministic validator decides what runs.

## Effort

**3-4 person-months** for one experienced developer, per the research estimate (concept § 5). The
total across P0-P5 is 17-21. The number assumes the chosen crates hold up and the UX scope does not
grow — treat a large overrun as a signal to re-cut, not to work longer.

## This is a phase, not a ticket

`size: epic
estimate_pm: "3-4"` is deliberate and it is a warning, not a label. This exceeds `L`, so it is **not
executable as one session and must not be started as one.** When P3 becomes next, cut it into
`EULEDB-SUB-<n>` tickets of size S to L before any work starts, then work those one at a
time. This file exists so the criteria below are visible in the backlog instead of being remembered.

## Criteria in scope

- closed typed IR enum, serde-serialisable (AC-40)
- fail-closed validator naming the rejected element (AC-41)
- destructive operations refused on the NL path by default (AC-42)
- plain-language restatement before execution, explanation after (AC-43, AC-44)
- instruction and data in structurally separate channels (AC-45)
- deterministic rule-based fallback parser (AC-46)
- local llama.cpp runtime on every supported platform, accelerators optional, no network on the query path (AC-47)
- correct-IR rate by MODEL SIZE, per-combination kill switch below 60 % (AC-48, AC-49, AC-50)

## Notes for the cut

**These criteria are the least grounded in the whole spec** and are expected to change once P2 is measured — the spec records that as a carried risk. Regenerate this phase against reality rather than against the plan. A natural-language benchmark set must be chosen before AC-48 and AC-50 mean anything.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this phase is cut. A ticket
detailed today against a repository state an earlier phase will change is wrong by the time it is
picked up — which is exactly why this file stays coarse.
