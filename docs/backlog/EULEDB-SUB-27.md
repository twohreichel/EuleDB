---
id: EULEDB-SUB-27
ticket: EULEDB
fulfils: []
depends_on: [EULEDB-SUB-26]
size: M
context_budget: 3000
safety: test fixtures and a documented corpus — no production path changes
detail: stub
status: backlog
---

## Goal

**Fix the reference corpus before any number is recorded.** Choose, document and vendor the reference corpus every later KPI is measured against, with a
brute-force baseline computed over it.

Fulfils no criterion of its own and gates four. AC-5 requires a third party to reproduce the numbers with
one documented command, and AC-2 compares recall against an exhaustive baseline **over the same corpus** —
so a corpus fixed after the first benchmark makes both unverifiable. The phase stub says this explicitly.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/specs/spec.md (AC-2, AC-5)`
- `docs/backlog/done/EULEDB-SUB-19.md` — the rows-examined measurement this reuses

## Notes for the cut

The corpus has to be multilingual, because the whole embedding choice is, and small enough to
live in the repository or be fetched by a pinned command. Licence matters: it ships with the benchmark.
Record its size, its languages and its provenance, and pin a checksum — a corpus that drifts silently
invalidates every recorded number without anyone noticing.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
