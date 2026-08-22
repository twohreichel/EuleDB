---
id: EULEDB-SUB-24
ticket: EULEDB
fulfils: [AC-6, AC-28]
depends_on: [EULEDB-SUB-14]
size: L
context_budget: 3000
safety: the gate is opt-in until a token is required — an ungated database keeps working
detail: stub
status: backlog
---

## Goal

**Gate access behind signed capability tokens, read-only by default.** Tokens carrying read, write or schema scope, signed with HMAC-SHA256 under a key derived from the
existing key-encryption key. Symmetric by decision: there is one party, so an issuer who can also verify
costs nothing, and it avoids a new dependency and a second key to manage.

Read-only is the default: a write, a schema change or a delete requires an explicitly granted scope.

**This changes the error surface.** A rejection must not reveal whether the target exists, and today a
missing table and an unauthorised one are distinguishable — `StorageError::Backend` names the table. Gated
operations need one indistinguishable refusal, and the test has to assert the two cases are identical,
message included.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/euledb-storage/src/error.rs`
- `crates/euledb-storage/src/store.rs`
- `crates/euledb-storage/src/crypto/keyring.rs`
- `docs/specs/spec.md (AC-6, AC-28)`

## Notes for the cut

Derive the signing key from the KEK with a distinct context string, so a token key and a data key
can never be the same bytes. `hmac` plus `sha2` from RustCrypto, matching the crates already in the tree.
Compare tags in constant time — a byte-wise comparison on a signature is a timing oracle.

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next. A
ticket detailed today against a repository state an earlier ticket will change is wrong by the time it is
picked up, which is why this stays coarse.
