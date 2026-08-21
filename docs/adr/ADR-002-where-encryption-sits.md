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
