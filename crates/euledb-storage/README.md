# euledb-storage

Storage layer for [`euledb`](https://crates.io/crates/euledb). Published because the facade crate
depends on it, not because it is meant to be used directly.

**Depend on `euledb` instead.** Everything here is an implementation detail: the on-disk format lives
behind a trait boundary precisely so that it stays replaceable, and a caller that reaches past the
facade would make it permanent. This crate carries no stability promise of its own.

## Readers, writers, and what happens after a crash

**Any number of readers, at most one writer, per database directory.**

- Opening for reading takes no lock and never waits. A writer does not block readers.
- Opening for writing takes the write role and holds it until the store is dropped. **A second writer is
  refused immediately**, with an error naming the database — not queued. A local-first database that
  blocks forever on a lock held by a process nobody can see is worse than one that says so.
- A writing call on a store opened for reading is refused, by name.

The role is an advisory lock on an open file handle, not a marker file. The difference shows up on a
crash: the operating system releases the lock when the process dies, however it dies, whereas a marker
would outlive the crash and lock the database out until somebody worked out which file to delete.

**An interrupted write leaves the database at the state before it or the state after it.** Each append is
one commit, so a writer killed mid-write leaves a whole number of appends behind — never a partial one.
That is tested rather than argued: a fixture process writes in a loop and is killed with no warning at
five different points, and the database is reopened and checked each time.

## Compression and string encoding

Stated rather than left to be discovered, because it decides how much space your data takes and there
is no own encoder involved:

- **Block compression is zstd, declared explicitly on every column, at level 1 by default** and
  configurable per table when the table is created.
- **String encoding is provided entirely by the Lance layer.** EuleDB contains no string encoder of its
  own and does not intend to write one. Lance chooses FSST or dictionary encoding itself, and it does it
  well: a repetitive multilingual corpus of 2.53 MB of text came out at 682 KB with nothing declared —
  a factor of 3.7, for no code.

Both statements come from a measurement you can repeat:

```bash
cargo run --release --example measure_encoding -p euledb-storage
```

On 20 000 rows of multilingual legal prose, three runs per configuration:

| Configuration | Data bytes | Stable across runs |
|---|---:|---|
| `compression = none` on the text columns | 2 749 453 | yes |
| nothing declared, Lance chooses | 681 622 | **no — varied by 97 KB** |
| FSST forced on the text columns | 886 244 | **no — varied by 44 KB** |
| **zstd level 1 on every column** | **649 029** | **yes** |
| zstd level 9 on every column | 647 813 | yes |
| zstd level 22 neighbourhood | 637 640 | yes |
| zstd level 1 on the text columns only | 673 518 | yes |

What the numbers decided:

- **Declare zstd rather than leave it to the format.** It is 5 % smaller than the format's best
  automatic run, and — the deciding argument — it is byte-identical across runs. The automatic choice
  varied by more than 20 % on identical input, and a stored size that moves on its own cannot be
  compared against a later one.
- **Level 1, not 9 or 22.** Level 22's neighbourhood is under 2 % smaller for several times the
  compression work, and the supported platforms include machines with four cores already running
  inference. Note that size is **not** monotonic in the level: level 3, zstd's own default, measured
  *larger* than level 1.
- **Every column, not only the text ones.** Declaring it everywhere came out 4 % smaller than declaring
  it on the string columns alone, which was not the expected result.
- **No own string encoder.** FSST forced by hand was *worse* than letting Lance decide, so Lance is
  doing something better than plain FSST — and writing an encoder to compete with that would be
  spending the project's scarcest resource on the one layer where it has no advantage.

Numbers taken on 2026-08-21 with `lance` 10.0.0 on aarch64-apple-darwin. Re-measure after a format
upgrade rather than trusting this table.

- Repository: <https://github.com/twohreichel/EuleDB>
- Licence: `Apache-2.0 OR MIT`
