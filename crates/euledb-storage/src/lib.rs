#![forbid(unsafe_code)]

//! Storage layer for EuleDB.
//!
//! Everything that knows the on-disk format lives here and nothing outside this crate names a type
//! from it. That boundary is the point of the crate: the format is a pinned, replaceable dependency,
//! and a leaked type would quietly make it permanent.
//!
//! Every fallible call returns [`Result`], and every failure is a value of [`Error`] — one type, so a
//! caller writes one `match` for the whole layer. Nothing here panics on bad input. The `# Errors`
//! section of each function names the *specific* failure it can produce, which is reached by matching
//! the [`Error`] variant that carries it.

mod compression;
mod crypto;
mod definition;
mod error;
mod mutation;
mod schema;
mod store;
mod writer_lock;

pub use compression::{Compression, InvalidZstdLevel, ZstdLevel};
pub use crypto::{DataKeyId, Keyring, KeyringError};
pub use definition::TableDefinition;
pub use error::{Error, Result};
pub use mutation::{Assignment, Deleted, Predicate, Updated};
pub use schema::{SchemaMismatch, TableSchema};
pub use store::{LanceStore, StorageError, TableStore};
