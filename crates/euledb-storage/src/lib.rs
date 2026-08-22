#![forbid(unsafe_code)]
// Computing the layout of the format's plan-analysis future costs 130 levels of query depth, and the
// default ceiling is 128. It only bites on Linux with a compiler newer than the pinned one — macOS and
// Windows stable compile it either way — so the matrix over two toolchains AND four platforms is what
// surfaced it. Raising the ceiling is the compiler's own remedy; boxing the future at the call site was
// tried first and only moved the cost inside the dependency, where it still has to be paid.
#![recursion_limit = "256"]

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

mod audit;
mod compression;
mod crypto;
mod definition;
mod embedding;
mod error;
mod fusion;
mod measurement;
mod mutation;
mod schema;
mod search;
mod store;
mod writer_lock;

pub use audit::{AuditError, AuditLog, AuditRecord};
pub use compression::{Compression, InvalidZstdLevel, ZstdLevel};
pub use crypto::{Capability, DataKeyId, Keyring, KeyringError, Scope};
pub use definition::TableDefinition;
pub use embedding::{Embedder, RowVector, StemmingLanguage, VECTOR_WIDTH, VectorIndexKind};
pub use error::{Error, Result};
pub use fusion::{DEFAULT_K, Fused, FusedHit, SMALL_CORPUS_K, SMALL_CORPUS_THRESHOLD};
pub use measurement::{Measured, Order, RowId, RowIdSet, RowsExamined};
pub use mutation::{Assignment, Deleted, Predicate, Updated};
pub use schema::{SchemaMismatch, TableSchema};
pub use search::CandidateSource;
pub use store::{LanceStore, StorageError, TableStore};
