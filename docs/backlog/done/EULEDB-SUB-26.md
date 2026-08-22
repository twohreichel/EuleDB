---
id: EULEDB-SUB-26
ticket: EULEDB
fulfils: [AC-30]
depends_on: [EULEDB-SUB-25]
size: M
context_budget: 3000
safety: verification is a read; the refusal to append is the only behaviour change, and it fails closed
detail: full
status: done
---

## Goal

**Verify the audit chain, and refuse to append past a broken link.** Report the index of the first link
that does not hold, and accept nothing further until the chain is explicitly re-anchored.

## What landed

`AuditLog::verify`, `AuditLog::reanchor`, and two error variants: `BrokenChain { at }` and
`NothingToReanchor`. `append` verifies before it writes, so a tampered log **fails closed** — one that
keeps accepting entries after it has been altered is worse than no log, because it still looks
trustworthy.

Three things break a link: content that no longer produces its own hash, a record that does not name its
predecessor, and a record that anchors a chain mid-log without saying it is a re-anchor.

## The design the first attempt got wrong

A log is a **sequence** of chains, not one chain. Re-anchoring starts a new one and leaves everything
before it in the file, because a recovery that erased the damage would be the one thing an audit log must
never do. The first implementation verified the whole file, so a re-anchored log stayed broken for ever
and appends never resumed — the test caught it immediately.

So verification asks about the **current** chain, and the break that ended the previous one is recorded
by the re-anchor itself. The gap stays visible: after re-anchoring past a removed record 2, the sequence
numbers read 0, 1, 3, 4 and a reader can see what happened.

## One test's expected value was wrong, and it was the test

`a_broken_chain_refuses_further_appends_until_it_is_re_anchored` asserted that the anchor names link 1 —
the record that was removed. The **first broken link** is 2, the first record whose predecessor is gone,
and that is what AC-30 asks for and what the other two tests already asserted. The test was corrected,
not the code, with the reasoning written at the assertion.

## The mutation pass found a laundering hole

Six mutations, five caught. **The sixth survived:** identifying the current chain by "the predecessor is
the anchor value" instead of by the re-anchor marker.

That is a real weakness, not a stylistic one. Anyone who can write the file could silence a break by
appending one bare record whose predecessor is the anchor value — verification would start reading from
there and every earlier record would go unexamined. The marker requirement is what prevents it, and it
now has a test that forges such a record, **hash included**, because a forgery with a wrong hash would be
caught by the content check instead and the test would pass for the wrong reason.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 138 tests
cargo +stable clippy --all-targets --all-features -- -D warnings
```

| Mutation | Caught by |
|---|---|
| the reported index is the position, not the sequence number | `verification_names_the_first_broken_link` |
| the content hash is not re-checked, only the links | `verification_names_the_first_broken_link` |
| append does not fail closed on a broken chain | `a_broken_chain_refuses_further_appends_until_it_is_re_anchored` |
| re-anchoring is allowed on a sound log | `re_anchoring_a_sound_log_is_refused` |
| the anchor does not record which link broke | `a_broken_chain_refuses_further_appends_until_it_is_re_anchored` |
| **any mid-log anchor is accepted, marker or not** | `an_anchor_without_the_marker_does_not_launder_the_break` (added for it) |

## Acceptance

- [x] AC-30 — the first broken link is reported by its sequence number, from the middle of a log rather
      than its ends, for both a tampered record and a removed one.
- [x] AC-30 — appends are refused until an explicit re-anchor, and the re-anchor records which link broke.
- [x] Re-anchoring a sound log is refused, so the operation cannot be used to obscure one.
- [x] The evidence survives re-anchoring: nothing before the break is removed, and the gap stays readable.
