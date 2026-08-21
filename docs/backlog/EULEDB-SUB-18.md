---
id: EULEDB-SUB-18
ticket: EULEDB
fulfils: [AC-22, AC-75]
depends_on: [EULEDB-SUB-12]
size: L
context_budget: 3000
safety: opt-in passphrase, the plaintext path is unchanged
detail: stub
status: backlog
---

## Goal

Encrypted data path. Every byte of table data at rest under AES-256-GCM with the data-encryption key of
AC-20, in independently addressable blocks so a range read does not decrypt the whole file, and failing
closed on a failed authentication tag with no partial plaintext.

## Why this is its own ticket

AC-20 originally bundled the key hierarchy with the data path. They are different sizes of problem: the
hierarchy is self-contained, this is security-critical composition behind the storage format's
object-store hook. One diff containing both would be a large piece of cryptography reviewed in a single
sitting, which is the arrangement most likely to let a defect through. See
`docs/adr/ADR-002-where-encryption-sits.md`.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `docs/adr/ADR-002-where-encryption-sits.md` — read first, it is the design
- `crates/euledb-storage/src/crypto/` (the key hierarchy from SUB-12)
- `crates/euledb-storage/src/store.rs`
- `docs/specs/spec.md` (AC-22, AC-75)

## The two things that must not be improvised

- **The nonce.** Derived from the object identity and the block index, never random and never a counter
  that could restart. That is what makes reuse structurally impossible instead of statistically
  unlikely, and nonce reuse in GCM is catastrophic rather than degrading.
- **The block size.** It becomes part of the on-disk layout, so it cannot change for an existing
  database. Choose it by measurement — read amplification against tag overhead — and record the numbers
  the way SUB-11 did for compression.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
