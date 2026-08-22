---
id: EULEDB-SUB-23
ticket: EULEDB
fulfils: [AC-27]
depends_on: [EULEDB-SUB-22]
size: M
context_budget: 3000
safety: a port with no real implementation yet — the fake in the test is its only consumer
detail: stub
status: backlog
---

## Goal

**Apply the exact filter as a pre-filter before candidate generation.** A query carrying both an exact filter and a search clause must narrow by the filter first and hand
the surviving row ids to candidate generation, not the other way round.

There is no candidate generator until P2, so what lands here is the seam: a driven port that receives the
pre-filter set, and a hand-written fake in the test that records what it was given. A port at an I/O
boundary is justified at one implementation, because it inverts the dependency rather than abstracting over
variants.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/backlog/done/EULEDB-SUB-22.md`
- `docs/specs/spec.md (AC-27)`

## Notes for the cut

The fake records the bitmap it received; the assertion is that its cardinality equals the filter's
matches, not the table's size. A mock asserting a call happened would prove nothing about the ordering the
criterion is actually about.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
