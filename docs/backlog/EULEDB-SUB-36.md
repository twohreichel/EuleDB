---
id: EULEDB-SUB-36
ticket: EULEDB
fulfils: [AC-72]
depends_on: [EULEDB-SUB-33]
size: M
context_budget: 3000
safety: documentation, with its examples compiled by the suite
detail: stub
status: backlog
---

## Goal

**A getting-started guide a newcomer can actually follow.** Install, create a table, insert, and run each of the three query kinds — exact filter, semantic,
full text — plus one hybrid query. **Every example compiled and executed by the test suite**, so the guide
cannot rot silently.

Release-blocking by the criterion's own words: a release without this is a library only its author can
use.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/specs/spec.md (AC-72)`
- `crates/euledb/src/database.rs` — the doc-example mechanics already in place
- `README.md`

## Notes for the cut

The examples must run, which rules out a hand-written Markdown file nobody compiles. The
established mechanism here is doc examples on the public API plus a page that includes them — SUB-14
already proved they catch an API change. Do not let the guide name a criterion id: the invariant test
forbids it, and rightly, because a contributor cannot read the specification.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
