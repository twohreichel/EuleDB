---
id: EULEDB-SUB-24
ticket: EULEDB
fulfils: [AC-6, AC-28]
depends_on: [EULEDB-SUB-14]
size: L
context_budget: 3000
safety: the gate is opt-in — the authority's own handle keeps working unchanged
detail: full
status: done
---

## Goal

**Gate access behind signed capability tokens, read-only by default.** Tokens carry read, write or schema
scope, and an operation whose token lacks the required scope is rejected without revealing whether the
target exists.

## What landed

`Scope`, `Capability`, `Keyring::grant` and `LanceStore::gated`. A gated handle may do only what a signed
token says; every operation checks its scope before touching anything.

HMAC-SHA256 under a key derived from the key-encryption key with its own context. Symmetric by decision
recorded at the P1 cut: this specification describes no second party, so an issuer that can also verify
costs nothing and avoids a key pair to manage.

## How AC-6's "default to read-only" is read, since a strict reader will ask

The gate is **opt-in at the handle**, and inside a gated handle the default is *nothing* — every
operation needs a token naming its table and its scope, so an empty grant list is a read-nothing handle
and a read token alone is read-only.

The authority's own handle is not gated, because the holder of the keyring **is** the authority: a
database with no restricted handle in it has nobody to restrict. This is the capability model as an
operating system uses it — a full handle, from which narrower ones are derived — and it is why every
existing test still passes unchanged.

## Two decisions, both against convenience

**Scopes are independent.** A write token does not permit reading and does not permit reshaping. Explicit
is worth the extra grant here: an implicit escalation in an authorisation model is exactly the
convenience that becomes a finding.

**The refusal is identical whether or not the table exists.** The check runs before the table is touched,
and the test asserts more than the variant — the rendered messages must be equal but for the name the
caller supplied, and neither may carry a cause the other does not. A differing chain is an oracle too.

## What the mutation pass found, and it was a real gap

Six mutations, five caught immediately. **The sixth survived:** signing tokens with the key-encryption
key itself instead of a derived key passed every behavioural test, because the gate still works. What it
would cost is the property that reading a block header or learning one data key tells an attacker nothing
about forging a token — a property with no observable behaviour.

So it is asserted directly, from inside the module: the token key equals neither the key-encryption key
nor any data key, and two keyrings do not share one. The mutation is caught now.

The length prefix in the signed message also earned an honest comment rather than a claim it does not
support: with three fixed scope words no table name plus one of them concatenates to another table name
plus another, so it is **not currently exploitable**. It costs eight bytes and makes the encoding
canonical before a free-form scope ever makes the argument necessary.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 124 tests
cargo +stable clippy --all-targets --all-features -- -D warnings
```

| Mutation | Caught by |
|---|---|
| the tag is never verified, so any token is honoured | `a_token_from_another_authority_is_not_honoured` |
| the table is not compared | `a_token_for_one_table_does_not_open_another` |
| the scope is not compared | `a_write_token_does_not_permit_reading` |
| the permission check runs after the table is opened | `a_refusal_does_not_reveal_whether_the_table_exists` |
| the table length is left out of the signed message | `a_token_for_one_table_does_not_open_another` |
| **the token key IS the key-encryption key** | `the_token_key_is_not_any_key_that_touches_data` (added for it) |

## Acceptance

- [x] AC-28 — read, write and schema scopes, each honoured only for the table its token names and only
      when the tag verifies under the authority that signed it.
- [x] AC-28 — a refusal reveals nothing about existence: same variant, same message but for the caller's
      own name, same absence of a cause.
- [x] AC-6 — a gated handle permits nothing it has no token for, so read-only is what a read token means.
- [x] Key separation asserted directly, since it has no observable behaviour.
- [x] The ungated authority handle is unchanged, which every pre-existing test confirms.
