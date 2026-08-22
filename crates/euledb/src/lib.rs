#![forbid(unsafe_code)]

//! Local-first embedded hybrid database.
//!
//! EuleDB fuses three retrieval paths — exact filters, vector semantics and BM25 full text — over one
//! encrypted file on disk, with no server and no network call on the query path.
//!
//! This crate is the public surface: [`Database`] and the types its six operations speak in.
//!
//! The interchange types come from Apache Arrow and are re-exported here, so a caller does not have to
//! pin the same arrow version by hand — a `RecordBatch` built against a different major is a different
//! type, not a convertible one.

mod config;
mod database;

pub use arrow_array;
pub use arrow_schema;
pub use config::Config;
pub use database::Database;
pub use euledb_storage::{
    Assignment, Compression, DataKeyId, Deleted, Error, Keyring, Predicate, Result, SchemaMismatch,
    StorageError, TableSchema, Updated, ZstdLevel,
};
