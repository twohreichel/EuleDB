---
id: EULEDB-SUB-26
ticket: EULEDB
fulfils: [AC-30]
depends_on: [EULEDB-SUB-25]
size: M
context_budget: 3000
safety: verification is a read; the refusal to append is the only behaviour change, and it fails closed
detail: stub
status: backlog
---

## Goal

**Verify the audit chain, and refuse to append past a broken link.** Walk the chain, and on the first link that does not verify report its index and refuse every further
append until the chain is explicitly re-anchored. Failing closed is the point: a log that keeps accepting
entries after it has been tampered with is worse than no log, because it still looks trustworthy.

Re-anchoring is an explicit operation that records the break it is anchoring past — otherwise the recovery
erases the evidence.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/backlog/done/EULEDB-SUB-25.md`
- `docs/specs/spec.md (AC-30)`

## Notes for the cut

Test by tampering: alter one record in the middle, assert the reported index is that link and not
the first or the last. An assertion that verification merely failed would pass with an off-by-one in the
index it reports, which is the number an operator acts on.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
