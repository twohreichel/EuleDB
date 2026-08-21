---
id: EULEDB-P5
ticket: EULEDB
kind: phase
fulfils: [AC-56, AC-57, AC-58, AC-59, AC-60, AC-61, AC-62, AC-73]
depends_on: [EULEDB-P4]
size: epic
estimate_pm: "3-4"
context_budget: 2000
safety: not a mergeable unit — see below
detail: stub
status: backlog
---

## Goal

**P5 — User experience and maturity.** The layer that makes EuleDB usable by someone who is not its author: visual blocks, friendly errors, Python bindings and published wheels.

## Effort

**3-4 person-months** for one experienced developer, per the research estimate (concept § 5). The
total across P0-P5 is 17-21. The number assumes the chosen crates hold up and the UX scope does not
grow — treat a large overrun as a signal to re-cut, not to work longer.

## This is a phase, not a ticket

`size: epic
estimate_pm: "3-4"` is deliberate and it is a warning, not a label. This exceeds `L`, so it is **not
executable as one session and must not be started as one.** When P5 becomes next, cut it into
`EULEDB-SUB-<n>` tickets of size S to L before any work starts, then work those one at a
time. This file exists so the criteria below are visible in the backlog instead of being remembered.

## Criteria in scope

- Blockly-style block editor structurally incapable of producing invalid IR (AC-56)
- all three query modes over one IR and one planner (AC-57)
- errors phrased without jargon, naming the next action (AC-58)
- PyO3 bindings with zero-copy Arrow via `pyo3-arrow` (AC-59)
- abi3 wheels for all four AC-11 platforms, smoke-tested (AC-60)
- a Class B platform tuned to meet AC-3 and AC-4, tuning documented and selectable, never auto-detected from a device name (AC-61)
- every public API item documented, doc examples executed by the suite — REFERENCE only (AC-62)
- **user documentation** in four parts: installation per AC-11 platform incl. model files,
  configuration reference for every AC-74 tunable with default and effect, task-oriented how-to
  guides per capability, and an explanation of the hybrid model (AC-73). Delete a section rather than
  pad it — an unfilled section is a liability, not a start.

## Notes for the cut

The Rust-first decision deferred PyO3 to here, which means abi3 constraints and Arrow zero-copy behaviour surface late. The spec carries that as an accepted risk — budget for surprises in AC-59 and AC-60.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this phase is cut. A ticket
detailed today against a repository state an earlier phase will change is wrong by the time it is
picked up — which is exactly why this file stays coarse.
