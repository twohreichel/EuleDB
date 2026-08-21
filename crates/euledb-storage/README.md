# euledb-storage

Storage layer for [`euledb`](https://crates.io/crates/euledb). Published because the facade crate
depends on it, not because it is meant to be used directly.

**Depend on `euledb` instead.** Everything here is an implementation detail: the on-disk format lives
behind a trait boundary precisely so that it stays replaceable, and a caller that reaches past the
facade would make it permanent. This crate carries no stability promise of its own.

- Repository: <https://github.com/twohreichel/EuleDB>
- Licence: `Apache-2.0 OR MIT`
