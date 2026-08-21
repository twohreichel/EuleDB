---
id: EULEDB-SUB-12
ticket: EULEDB
fulfils: [AC-20, AC-22]
depends_on: [EULEDB-SUB-11]
size: L
context_budget: 3000
safety: opt-in passphrase, plaintext path unchanged
detail: stub
status: backlog
---

## Goal

Encryption at rest. Argon2id key-encryption key from the passphrase, AES-256-GCM payload encryption under a wrapped data-encryption key, failing closed on a wrong passphrase or a failed auth tag with no partial plaintext.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `crates/*/src/crypto/ (new)`
- `crates/*/src/storage/`
- `docs/specs/spec.md (AC-20, AC-22)`
- `docs/specs/spec.md § Glossary (KEK/DEK)`

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
