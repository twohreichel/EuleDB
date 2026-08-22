#![forbid(unsafe_code)]

//! Storage layer for EuleDB.
//!
//! Everything that knows the on-disk format lives here and nothing outside this crate names a type
//! from it. That boundary is the point of the crate: the format is a pinned, replaceable dependency,
//! and a leaked type would quietly make it permanent.

mod compression;
mod crypto;
mod definition;
mod mutation;
mod schema;
mod store;
mod writer_lock;

pub use compression::{Compression, InvalidZstdLevel, ZstdLevel};
pub use crypto::{DataKeyId, Keyring, KeyringError};
pub use definition::TableDefinition;
pub use mutation::{Assignment, Deleted, Predicate, Updated};
pub use schema::{SchemaMismatch, TableSchema};
pub use store::{LanceStore, StorageError, TableStore};
pub use writer_lock::{LockError, WriteLock};
