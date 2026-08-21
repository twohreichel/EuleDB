---
id: EULEDB-SUB-18
ticket: EULEDB
fulfils: [AC-22, AC-75]
depends_on: [EULEDB-SUB-12]
size: L
context_budget: 3000
safety: opt-in — a store without a keyring behaves exactly as before
detail: full
status: done
pr: https://github.com/twohreichel/EuleDB/pull/13
---

## Goal

Encrypted data path. Every byte of table data at rest under AES-256-GCM with the data-encryption key of
AC-20, in independently addressable blocks so a range read does not decrypt the whole file, and failing
closed on a failed authentication tag with no partial plaintext.

## Context (read ONLY these files)

- `docs/adr/ADR-002-where-encryption-sits.md` — the design AND both amendments
- `crates/euledb-storage/src/crypto/{frame,store,provider}.rs`
- `crates/euledb-storage/src/store.rs`
- `crates/euledb-storage/tests/encrypted.rs`
- `docs/specs/spec.md` (AC-22, AC-75)

## The first mechanism was wrong, and the tests said so

ADR-002 chose the format's `object_store_wrapper` hook. **It is not on the path a local data file takes.**
`lance_io::object_store::ObjectStore::create` dispatches on the URI scheme and the `"file"` branch writes
through `tokio::fs` without touching the `object_store` trait.

Measured with the wrapper installed and every call logged: it was consulted for `list` and for the
manifest `put`, **no data-file write reached it**, and the data file carried the row text in the clear.
The test that caught it: opening with a *different* keyring returned **2000 rows successfully**.

## The mechanism that works

**A provider registered for a private URI scheme.** Under a scheme that is not `file`, `is_local()` is
false, `create` takes the `ObjectWriter` branch, and every byte goes through the trait.

- `EncryptingProvider` implements `ObjectStoreProvider` for scheme `euledb`, returning a
  `lance_io::ObjectStore` built on `EncryptingObjectStore` over `LocalFileSystem`.
- Its own `ObjectStoreRegistry` in its own `Session`, not the process-wide default: the registry carries
  this database's cipher, and a shared one would hand one database's key to another.
- `LanceStore::encrypted(&keyring)` switches the URIs to that scheme and threads the session through
  both the write path (`WriteParams::session`) and the read path (`DatasetBuilder::with_session`).

`lance-io` became a direct dependency, pinned to the same exact version as `lance`, because `lance` does
not re-export the provider trait — and a differently-versioned trait is a different trait.

## Verified by reading the disk, not by asserting

Every object the format writes is framed — the data file, both manifests, the transaction records, and
the version hint. `cargo run --release --example inspect_encrypted -p euledb-storage` prints it.

Five tests, and the one that carries the others is the **control**:

| Test | Why it exists |
|---|---|
| the marker IS on disk without encryption | without it, the next test passes against a table that was never encrypted — which is exactly what happened first, because zstd had already made the marker unfindable |
| the marker is NOT on disk with encryption | the actual claim |
| rows survive a drop and reopen unchanged | the round trip, reopened from the keyfile the way a caller would |
| another key cannot read the table | this returned 2000 rows against the first mechanism |
| a plaintext table is not read as encrypted | the layer must refuse foreign bytes rather than interpret them |

## The manifest size question is answered

It does not recur. The mismatch came from a **partially** bypassed store — the manifest went through the
layer while its size was observed on the raw object, because the data path had gone around entirely. With
one consistent path there is one size. Stated as a rule: **a translating layer must be on every path or
on none.**

## The cost, measured

Best of five, compression off so the numbers are about encryption alone:

| Rows | Size overhead | Round trip |
|---:|---:|---|
| 20 000 | +0.06 % | 1.01x |
| 200 000 | +0.04 % | 2.75x |
| 1 000 000 | +0.04 % | 3.09x |

Size is negligible — 28 bytes per 64 KiB block is 0.043 % and the measurement matches. **Time is roughly
3x on a write-plus-read round trip at scale**, bundling the cipher work and the lost local fast path,
which this measurement cannot separate. See the ADR for the reasoning about which dominates.

## One more finding: the read path was not validating the header

It worked — a plaintext object failed on the tag — but the message was "block 0 did not authenticate"
rather than "this object is not encrypted by EuleDB", and three error variants were unreachable. The
compiler's dead-code warning found it. Every read now fetches the header alongside the block span in one
`get_ranges` call, at no extra round trip.

## Verification (executable)

```bash
just format && just lint && just test && just qa

# what is actually on disk, and what encryption costs
cargo run --release --example inspect_encrypted -p euledb-storage

# the framing's own guarantees — break one, a test notices
#   block index dropped from the AAD     -> reordering accepted
#   final marker dropped                 -> a forged final block accepted
#   nonce fixed                          -> identical ciphertext
#   span not clamped                      -> a read near the end runs off the object
#   magic / version / block-size check removed -> a foreign object silently read
#   range-completeness check removed     -> a short answer instead of an error
```

## Out of scope / Guardrails

- **The block size stays fixed at the default.** Making it settable first requires deciding whether a
  reader adopts the size declared in an object's header or refuses a mismatch, and that belongs with
  AC-74's single configuration mechanism. The 64 KiB default is justified on the numbers in hand.
- **No unauthenticated mode**, however tempting a length-preserving cipher looks for removing the size
  translation. AC-22 requires failing on a failed tag, and a stream cipher with no tag cannot.
- **Never leave an object unencrypted for convenience.** Excluding metadata was tried as a diagnostic and
  it leaks the schema and row counts. It was a diagnostic, not a design.
- **A translating layer goes on every path or none.** Half of one is what produced the manifest mismatch.

## Definition of Done

- [x] AC-75 covered: every object encrypted, proven by reading the bytes off disk, with a control test
- [x] AC-22 covered: a failed tag yields no plaintext, another key cannot read, a foreign object refused
- [x] The manifest size question answered rather than worked around
- [x] The cost measured and recorded, including what the number bundles
- [x] Every framing guarantee shown to be guarded by a test that fails when it is undone
- [x] The block size decision named and deferred with its reason, not silently left
