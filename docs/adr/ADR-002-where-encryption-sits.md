# ADR-002 — Where encryption at rest sits

- **Date:** 2026-08-21
- **Status:** Accepted

## Context

The database must encrypt everything it writes with AES-256-GCM under a rotatable data-encryption key
(AC-20, AC-75), while keeping the random access the on-disk format was chosen for in the first place
(ADR-001). Those two requirements pull against each other, and where the encryption is applied decides
whether both survive.

Two facts settled the search, both established by reading the format's source rather than its
documentation:

1. **The format has no encryption of its own.** Not in its file layer, not in its encoding layer, not in
   its I/O layer. The only occurrences of the word are unrelated cloud secret-manager options.
2. **It has a deliberate seam.** `ObjectStoreParams` carries an `object_store_wrapper` field taking a
   `WrappingObjectStore`, and the registry accepts a provider per URI scheme. Every byte the format
   reads or writes passes through that layer.

The hard constraint is that AES-GCM is not seekable. Encrypting a file as one AEAD message means any
read decrypts the whole file, which destroys random access — the property the format exists to provide.

## Decision

**Encrypt in a wrapping object store, with block-framed AES-256-GCM.**

- An encrypting layer implements `object_store::ObjectStore` and wraps the real one, installed through
  the format's `object_store_wrapper` hook. The format is unaware of it, so nothing in the storage
  boundary of AC-17 changes.
- Plaintext is framed into **fixed-size blocks**, each sealed independently with its own nonce derived
  from the object identity and the block index. A read of a plaintext range maps to the covering
  ciphertext blocks, which are the only ones fetched and decrypted.
- The **key hierarchy is a separate, smaller problem** and is built first: Argon2id derives a
  key-encryption key from the passphrase, which wraps the data-encryption key. Rotation re-wraps the
  data-encryption key and never rewrites payload (AC-21).

The nonce construction is the part that must not be improvised. Deriving it from `(object id, block
index)` rather than at random is what makes nonce reuse structurally impossible rather than
statistically unlikely, and the framing follows the shape used by established streaming-AEAD designs
rather than a new one.

## Consequences

**Positive.**
- Random access survives. A range read touches the blocks it needs, not the file.
- The format stays unaware of encryption, so it remains as replaceable as ADR-001 requires.
- The key hierarchy is testable on its own, without any data path, which is why it ships first.
- Rotation is cheap by construction: re-wrapping a key touches key material only.

**Negative, and accepted.**
- **A block size is now a permanent trade-off.** Small blocks mean fine-grained reads and more tag
  overhead; large blocks mean the opposite. It has to be chosen by measurement and then never changed
  for an existing database, because it is part of the on-disk layout.
- **Tag overhead is real.** Every block carries a 16-byte tag and a nonce, so a database of many small
  blocks pays a percentage of its size for authentication.
- **This is cryptographic composition, and composition is where mistakes live.** The primitives are
  audited; the framing around them is ours. It is written against an existing design, kept in one
  module, and it is the reason `SECURITY.md` says the design has not been independently audited.
- **Encryption happens after compression**, which is the correct order — compressing ciphertext
  achieves nothing — but it means the compression ratio is observable in the ciphertext size. For a
  local-first database whose file sits on the owner's disk this is accepted. It would not be acceptable
  for a service encrypting many tenants into one artifact.

## Alternatives considered

**One AEAD message per file.** Simplest to get right, and it is what a naive reading of "encrypt the
file" produces. Rejected: no seekable read, so every random access decrypts an entire file and the
reason for choosing the format in the first place is gone.

**Decrypt the whole database into a working copy on open.** Trivially correct and used by some desktop
applications. Rejected: it doubles disk, writes plaintext to a temporary location, and the plaintext
outlives a crash — which is the opposite of what an encrypted database is for.

**Encrypt only some columns.** Cheaper, and it keeps indexes on the clear columns fast. Rejected: it
makes "encrypted at rest" a claim with a footnote, and the footnote is the part users do not read.

**Wait for the format to add encryption.** It may. Rejected as a plan: the criteria exist now, and a
dependency's roadmap is not a design. Should it arrive, the wrapping layer is one module to delete —
which is an argument for this shape rather than against it.

## Follow-on

The decision splits the work in two, which is why AC-20 was narrowed and AC-75 appended (see the
specification's § Decisions taken). The key hierarchy lands in `EULEDB-SUB-12`, the encrypted data path
in `EULEDB-SUB-18`.

---

## Amendment, 2026-08-21 — the chosen mechanism does not reach the data files

**Status of the decision above: the placement is still right, the mechanism is wrong.** Encryption does
belong in a layer the format writes through, and block framing is still what makes a range read
possible. But `object_store_wrapper` is **not** that layer on a local filesystem, which is the only
platform this project targets.

### The evidence

`lance_io::object_store::ObjectStore::create`, the writer every data file is written with, dispatches on
the URI scheme:

```rust
pub async fn create(&self, path: &Path) -> Result<Box<dyn Writer>> {
    match self.scheme.as_str() {
        "file" => {
            // tokio::fs::File, a NamedTempFile, and a LocalWriter
            Ok(Box::new(LocalWriter::new(file, path.clone(), temp_path, ...)))
        }
        _ => Ok(Box::new(ObjectWriter::new(self, path).await?)),
    }
}
```

The `"file"` branch never touches the `object_store` trait, so a wrapper installed on it is not on the
path. The same is true of `copy_file` and `remove_dir_all`, both behind `is_local()`.

Measured, not inferred. With the wrapper installed and logging every call:

- the wrapper was constructed five times and *was* consulted for `list` and for the manifest `put`,
- **no data-file write reached it at all**, and
- the data file on disk began with no framing magic and **contained the row text in the clear**.

The test that caught it is the one worth keeping: opening the table with a *different* keyring returned
**2000 rows successfully**. A layer that looks like encryption and is not is worse than none, because it
changes what people are willing to store.

Two further observations, one of them still unexplained:

- **A compression-only control is essential.** The first "the plaintext is not on disk" test passed
  against a completely unencrypted table, because zstd had already made the marker string unfindable.
  Without the control asserting the marker *is* visible without encryption, that test proved nothing.
- **Manifest sizes collide with size translation, and the cause is not yet understood.** Reporting the
  plaintext size for a manifest produced `Invalid range 0..611 for object of size 574 bytes` from a
  store named `memory` — a size taken on the raw object combined with content returned through the
  layer. It is recorded here rather than guessed at, because building the corrected mechanism on top of
  an unexplained failure is how a security defect ships.

### The corrected mechanism

**Register a provider for a custom URI scheme instead of wrapping the store.**
`ObjectStoreRegistry::insert(scheme, provider)` and `ObjectStoreProvider::new_store` are public, and
`lance_io::object_store::ObjectStore::new` accepts an arbitrary `Arc<dyn object_store::ObjectStore>`
together with the `Url` that decides the scheme. With a scheme of, say, `euledb`, `is_local()` is false
and `create` takes the `ObjectWriter` branch — through the trait, and therefore through the encrypting
layer.

Its costs, which are real and belong in the decision:

- **The format's local fast paths are given up**: the direct `tokio::fs` writer, `copy_file` and
  `remove_dir_all`. Everything goes through the generic object-store path. That is a performance cost to
  be measured, not assumed away.
- **The registry has to be threaded through both the read and the write path**, which happens via a
  `Session` rather than through `ObjectStoreParams`.
- **The manifest size question returns** and has to be answered before anything is wired up.

### What was kept, and what was deliberately not

`crates/euledb-storage/src/crypto/frame.rs` — the block framing — is complete and carries 36 tests, and
the object-store layer that sits on it is written. Neither is wired in, and the crypto module says so at
the top with this amendment cited. Removing them would discard the part that is correct; wiring them
would publish a claim that is false.

`EULEDB-SUB-18` returns to the backlog with this design, rather than being closed.

---

## Amendment, part two — the correction works, and what it costs

Implemented and measured. **Every object the format writes is now encrypted**, verified by reading the
bytes off disk rather than by asserting it:

```
t.lance/data/0100110000000110…lance   1296073 bytes  framed=true
t.lance/_versions/…4.manifest              454 bytes  framed=true
t.lance/_versions/latest_version_hint.json  50 bytes  framed=true
t.lance/_transactions/1-….txn              162 bytes  framed=true
```

The five tests that matter all pass, including the two that failed against the first attempt: a foreign
keyring cannot read the table, and a plaintext table is refused rather than misread. The control test —
the marker string **is** findable on disk without encryption — passes too, and it is what makes the
others mean anything.

### The manifest size question is answered

It does not recur. The mismatch came from a **partially** bypassed store: the manifest went through the
layer while its size was observed on the raw object, because the data path had gone around the layer
entirely. With one consistent path there is one size, and it is the plaintext one throughout.

That is worth stating as a rule rather than an anecdote: **a translating layer must be on every path or
on none.** Half of one produces two truths about the same object, and the failure surfaces far from the
cause.

### The cost, measured

Best of five runs, compression off so the numbers are about encryption alone:

| Rows | Plaintext | Encrypted | Size overhead | Round trip |
|---:|---:|---:|---:|---|
| 20 000 | 1 296 685 | 1 297 439 | +0.06 % | 4.0 ms → 4.1 ms (1.01x) |
| 200 000 | 13 150 838 | 13 156 660 | +0.04 % | 16.3 ms → 44.7 ms (2.75x) |
| 1 000 000 | 66 191 545 | 66 220 017 | +0.04 % | 64.6 ms → 199.6 ms (3.09x) |

**Size overhead is negligible** — 28 bytes per 64 KiB block is 0.043 %, and the measurement matches.

**Time is roughly 3x on a write-plus-read round trip at scale**, and that is the honest headline. It
bundles two things this measurement cannot separate: the AES-GCM work itself, and the loss of the
format's local fast path. A reasoned estimate, offered as an estimate: 66 MB written and read again is
132 MB through the cipher, which at AES-NI speeds is a large share of the 135 ms difference — so most of
it is likely inherent to encrypting the data rather than an artefact of the mechanism. Separating them
would need a knob that exists only to be measured, which is why it has not been added.

### One further finding: the read path was not validating the header

The first working version translated sizes and decrypted blocks without ever looking at the header. It
*worked* — a plaintext object still failed, on the authentication tag — but the diagnosis was "block 0
did not authenticate" rather than "this object is not encrypted by EuleDB". Three error variants were
unreachable, which is how the compiler's dead-code warning found it.

Every read now fetches the header alongside the block span in one `get_ranges` call and validates it.
That is the difference between a diagnosis and a shrug, and it costs no extra round trip.

### Left open, deliberately

**The block size is not configurable, and the measurement that would justify a different default has not
been taken.** Taking it end to end requires making the size settable, which requires deciding whether a
reader *adopts* the size declared in an object's header or *refuses* a mismatch. That is a configuration
decision and belongs with AC-74's single mechanism, not here. The current default of 64 KiB is defensible
on the numbers already in hand: overhead at that size is 0.04 %, a 4 KiB block would raise it to roughly
0.7 %, and a larger block amplifies every small read.
