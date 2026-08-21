---
id: EULEDB-SUB-18
ticket: EULEDB
fulfils: [AC-22, AC-75]
depends_on: [EULEDB-SUB-12]
size: L
context_budget: 3000
safety: NOT SHIPPED — the mechanism does not reach the data files, see below
detail: full
status: blocked
---

## Goal

Encrypted data path. Every byte of table data at rest under AES-256-GCM with the data-encryption key of
AC-20, in independently addressable blocks so a range read does not decrypt the whole file, and failing
closed on a failed authentication tag with no partial plaintext.

## Status: blocked, and why

**The mechanism ADR-002 chose does not reach the data files on a local filesystem**, which is the only
platform this project targets. `lance_io::object_store::ObjectStore::create` dispatches on the URI
scheme and the `"file"` branch writes through `tokio::fs` with a `LocalWriter`, never touching the
`object_store` trait. A wrapper installed on that store is simply not on the path.

Measured with the wrapper installed and every call logged: it was consulted for `list` and for the
manifest `put`, **no data-file write reached it**, and the data file on disk began with no framing magic
and carried the row text in the clear. The test that caught it: opening with a *different* keyring
returned **2000 rows successfully**.

Nothing was shipped as a result. The evidence, the two secondary findings and the corrected mechanism
are in `docs/adr/ADR-002-where-encryption-sits.md` § Amendment.

## What is done, and correct

`crates/euledb-storage/src/crypto/frame.rs` — the block framing. Complete, 36 tests, every mutation
caught, and it is the part the corrected mechanism reuses unchanged:

- header carrying magic, version and block size, so a reader learns the layout from the object,
- a random nonce per block, because a path-derived nonce breaks the moment the format renames or copies
  an object, which it does on every commit,
- the block index and a final-block marker as authenticated data, so reordering and truncation fail,
- every object ending on a block shorter than a full one, which is what makes "final" readable from a
  block's length.

`crates/euledb-storage/src/crypto/store.rs` — the object-store layer: size translation, range
translation, and a multipart writer that seals whole blocks and holds the tail back until completion. It
is written and it compiles, and it is **not wired in**. The crypto module says so at the top.

## What has to happen next

1. **Register a provider for a custom URI scheme** rather than wrapping the store.
   `ObjectStoreRegistry::insert(scheme, provider)`, `ObjectStoreProvider::new_store`, and
   `lance_io::object_store::ObjectStore::new` taking an arbitrary store plus the `Url` that decides the
   scheme. With a non-`file` scheme, `is_local()` is false and `create` goes through the trait.
2. **Answer the manifest size question first.** Reporting the plaintext size for a manifest produced
   `Invalid range 0..611 for object of size 574 bytes` from a store named `memory` — a size taken on the
   raw object combined with content returned through the layer. The cause is not yet understood.
   **Do not build on top of it until it is.**
3. **Measure the cost of giving up the local fast paths** — the direct writer, `copy_file`,
   `remove_dir_all` — the way SUB-11 measured compression. It is a performance cost, not an assumption.
4. **Choose the block size by measurement**, read amplification against per-block overhead. The default
   is currently 64 KiB, chosen by nothing.

## Two findings worth carrying forward regardless

- **A compression-only control is essential to any "the plaintext is not on disk" test.** The first
  version passed against a completely unencrypted table, because zstd had already made the marker string
  unfindable. Without a control asserting the marker *is* visible without encryption, that test proves
  nothing.
- **Read the dependency's dispatch, not its extension points.** The hook exists, is documented, is
  called, and is not on the path that matters. Only running it with logging showed that.

## Out of scope / Guardrails

- **NEVER wire the crypto layer up until step 2 is answered.** A layer that looks like encryption and is
  not is worse than none, because it changes what people are willing to store.
- **No unauthenticated mode**, however tempting length-preserving encryption looks for removing the size
  translation entirely. AC-22 requires failing on a failed authentication tag, and a stream cipher with
  no tag cannot.
- **Do not encrypt only some objects.** Excluding metadata was tried as a diagnostic and it does leak
  the schema and row counts. It is a diagnostic, not a design.

## Definition of Done

- [ ] AC-75 covered: every byte of table data at rest encrypted, proven by a test that finds the marker
      string on disk WITHOUT encryption and fails to find it WITH
- [ ] AC-22 covered: a failed tag yields no plaintext, and another key cannot read the table
- [ ] The manifest size question answered, not worked around
- [ ] The block size chosen by a recorded measurement
- [ ] The cost of the generic object-store path measured
