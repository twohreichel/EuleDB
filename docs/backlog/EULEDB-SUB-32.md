---
id: EULEDB-SUB-32
ticket: EULEDB
fulfils: [AC-36]
depends_on: [EULEDB-SUB-27]
size: L
context_budget: 3000
safety: a separate index and a new query path — the vector side is untouched
detail: stub
status: backlog
---

## Goal

**BM25 full text with stable ranking.** A BM25 query answered through Tantivy, with ranking stable across identical runs.

**Worth settling first:** the format also ships an inverted index. Tantivy is `set` in the stack table for
a stated reason — stemming across 17 Latin languages — and this database is multilingual, so that is
likely decisive. Check what the format's inverted index does for stemming before accepting a second
full-text engine in the tree, and write the comparison down either way.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/specs/spec.md (AC-36)` and § Technology stack
- `crates/euledb-storage/src/search.rs` — the port it plugs into
- `docs/backlog/done/EULEDB-SUB-27.md`

## Notes for the cut

Stable ranking is the assertion. Ties are where it breaks: two documents with equal scores must
come back in the same order every run, which usually means a documented tie-break rather than whatever the
engine happens to do.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
