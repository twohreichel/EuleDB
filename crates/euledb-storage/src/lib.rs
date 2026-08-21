#![forbid(unsafe_code)]

//! Storage layer for EuleDB.
//!
//! Everything that knows the on-disk format lives here and nothing outside this crate names a type
//! from it. That boundary is the point of the crate: the format is a pinned, replaceable dependency,
//! and a leaked type would quietly make it permanent.

mod schema;

pub use schema::{SchemaMismatch, TableSchema};
