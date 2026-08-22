---
id: EULEDB-SUB-17
ticket: EULEDB
fulfils: [AC-71]
depends_on: [EULEDB-SUB-16]
size: M
context_budget: 3000
safety: defines the surface every later ticket returns through
detail: full
status: done
pr: https://github.com/twohreichel/EuleDB/pull/17
---

## Goal

Public error type. One documented error type for every failure. The public API must not panic on
malformed input, a missing or unreadable file, a permission error or a failed decryption — a library
that aborts its host process on bad data cannot be embedded.

## What landed

`Error` in `crates/euledb-storage/src/error.rs`, `#[non_exhaustive]`, four variants each carrying the
specific failure from the part of the system that produced it, plus `Result<T>`. Every public fallible
signature in the layer now returns it: `TableStore`'s five methods, `LanceStore::open_for_writing`,
`TableSchema::validate`, `Keyring::{create, open, rotate_data_key, change_passphrase}` and
`ZstdLevel::new`.

Every variant is `#[error(transparent)]`, so `Display` passes through to the failure that actually
happened and a caller who only logs the error never reads a category name. The specific failure is
recovered by matching the variant — `transparent` forwards `source()` too, so the wrapper is invisible
in the chain by design.

The ticket's context guessed `crates/euledb/src/error.rs`. That crate is deliberately still empty, so
the type lives in the storage layer and the facade re-exports it in SUB-14.

## Not this ticket, and worth a ticket of its own

A missing table, an unreadable file and a failed decryption all arrive as `StorageError::Backend`. The
criterion asks that none of them panic, and none of them does — but a caller cannot tell "wrong key"
from "no such table", and for a decryption failure that is worth telling apart. Reported rather than
fixed here: distinguishing them means classifying the backend's own error, which is a behaviour change
the criterion does not ask for.

## Removed

`Error::Lock` was unreachable. `LanceStore::open_for_writing` translates every `LockError` into a
`StorageError`, so no public call could ever return it — a documented variant nobody can observe. With
it went the incidental `pub use writer_lock::{LockError, WriteLock}` (added as a by-product of
`c0a9181`, referenced by no caller, no test and no document) and the `WriteLock::root()` accessor plus
the field it read, which were only live because the type was public.

`ZstdLevel::new` lost `const`: `Into::into` is not const. No caller used it in a const context, and a
fallible constructor is barely usable in one — a uniform error surface is worth more than a `const fn`
nobody can call.

## Verification

```bash
just format && just lint && just test && just qa   # all green, 79 tests
```

Five mutations applied by hand, all caught:

| Mutation | Caught by |
|---|---|
| `Display` becomes a category label instead of the failure | `the_one_type_renders_as_the_failure_it_carries` |
| the backend cause is no longer a `#[source]` | `assert_refused`, in every case |
| a missing table panics instead of returning | `no_public_call_panics_on_bad_input` |
| a wrong keyfile version is reported as merely malformed | `no_public_call_panics_on_bad_input` |
| the keyfile header length check is removed | `no_public_call_panics_on_bad_input` |

## Acceptance

- [x] AC-71 — one documented error type, and the four cases it names each have a test:
      malformed input, a missing file, a permission problem, a failed decryption.
- [x] No assertion is `is_err()`. Every case matches the variant and the table it names, and
      `assert_refused` also pins that the cause below stays reachable.
- [x] The two doc examples the signature change broke were fixed rather than skipped.
