---
id: EULEDB-SUB-13
ticket: EULEDB
fulfils: [AC-21]
depends_on: [EULEDB-SUB-18]
size: M
context_budget: 3000
safety: additive API — a store that never rotates behaves exactly as before
detail: full
status: in-progress
---

## Goal

Rotate the data-encryption key by re-wrapping, without rewriting the encrypted payload, and keep
previously written data readable.

## Context (read ONLY these files)

- `crates/euledb-storage/src/crypto/{keyring,frame}.rs`
- `crates/euledb-storage/tests/{keyring,rotation}.rs`
- `docs/specs/spec.md` (AC-21) and `docs/adr/ADR-002-where-encryption-sits.md`

## The criterion says two things, and both were built

"WHEN the data-encryption key is rotated, THE SYSTEM SHALL re-wrap the DEK without rewriting the
encrypted payload, and previously written data SHALL remain readable."

Read literally, "re-wrap the DEK" is a **passphrase change**: the same data key, wrapped under a new
key-encryption key. Read by its own title, it is a **data-key rotation**: a new key for new writes, with
the old one retained — and only then does "previously written data shall remain readable" carry any
weight, because under a passphrase change it is trivially true.

Both are real operations and both are implemented. The rotation is the substantial one and the passphrase
change falls out of the same key set for about ten lines.

## What changed on disk, and why it had to

**Both formats went to version 2.** Nothing has been released, so this costs nothing, and bumping rather
than redefining version 1 is the habit worth keeping.

- **Keyfile v2 holds a set of keys**, not one: `version | salt | nonce | sealed(current_id, count,
  [id|key]*)`. One AEAD message over the whole set, so the ids and the current pointer are authenticated
  along with the keys. The file is now variable-length — 36 bytes per rotation.
- **Framing v2 carries the key id in the header**, and in the authenticated data. That is what makes
  rotation possible without rewriting: an object records which key sealed it, so a rotated keyring still
  opens it, and pointing an object at a different key fails the tag rather than reading as something else.

## Two decisions worth arguing with

**A retired key is kept, never discarded.** Discarding it would make every row it sealed unreadable,
which is data loss wearing a security measure's clothes. The cost is that old data keeps whatever
protection its key had — rotation limits future exposure, it does not repair past exposure.

**The key-encryption key is held in memory** so that rotating does not need the passphrase again.
Argon2id is deliberately expensive, and demanding it on every write would push callers into caching it
themselves, less carefully. It is zeroized on drop like every other key here.

## TDD record, and the gap the mutation pass found

Tests written first, all RED on the missing type or method. Then the mutation pass:

| Mutation | Caught | |
|---|---|---|
| reads always use the current key | 2 tests | the retired key is genuinely used to read old objects |
| the header lies about which key sealed it | 1 test | |
| rotation does not adopt the new key | 2 tests | |
| rotation reuses the old key material | 1 test | |
| **changing the passphrase keeps the old salt** | **no** | reusing the salt lets a precomputation against the old passphrase carry over. Nothing noticed. A test now pins that both the salt and the nonce change |

The keyfile tests also had to change with the format, and the change is worth naming: a variable-length
file cannot be checked against one expected size, so a truncated keyfile was falling through to the
authentication tag and being reported as **a wrong passphrase** — which would send someone to re-type a
passphrase that was correct. The shape is still fixed (a tag, a set header, a whole number of entries),
so it is checked structurally and reported as damage.

## Verification (executable)

```bash
just format && just lint && just test && just qa

# the criterion's own two claims, each its own test
cargo nextest run -p euledb-storage -E 'binary(rotation)'
#   rotating leaves earlier rows readable      — 1000 rows across two keys
#   rotating rewrites no payload               — data files compared byte for byte, before and after
#   changing the passphrase leaves data intact — payload unchanged, still readable under the new one
```

## Out of scope / Guardrails

- **Never drop a retired key.** Everything it sealed becomes unreadable, and nothing warns.
- **Never derive a data key from the passphrase.** It would make either rotation a full rewrite.
- **No re-encryption command.** Rotating limits future exposure. Re-sealing old payload under a new key
  is a different, expensive operation and nobody has asked for it.
- The keyring grows by 36 bytes per rotation. That is fine for any realistic number and it is not a
  reason to prune.

## Definition of Done

- [ ] AC-21 covered both ways: a data-key rotation and a passphrase change, neither rewriting payload
- [ ] Earlier rows proven readable after a rotation, by row ids rather than by row count
- [ ] "No payload rewritten" proven by comparing bytes, not by trusting a timestamp
- [ ] Every mutation of the rotation path shown to be caught, and the one gap closed
- [ ] Both on-disk formats versioned rather than redefined
