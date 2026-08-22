//! The one error type every failure comes back as.

use crate::compression::InvalidZstdLevel;
use crate::crypto::KeyringError;
use crate::schema::SchemaMismatch;
use crate::store::StorageError;

/// Everything that can go wrong, in one type.
///
/// A caller writes one `match` and covers the whole database. Nothing here panics: a malformed batch, a
/// missing or unreadable file, a permission problem and a failed decryption all arrive as values,
/// because a library that aborts its host process on bad data cannot be embedded in anything.
///
/// The variants are thin. Each carries the specific error from the part of the system that produced it,
/// and its `Display` passes straight through — so `error.to_string()` describes what happened rather
/// than which module it happened in, and [`Error::source`](std::error::Error::source) reaches the detail
/// for a caller who wants to distinguish cases.
///
/// `#[non_exhaustive]`, because a database grows failure modes and adding one must not break a caller's
/// build.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A record batch did not match the table's declared schema.
    #[error(transparent)]
    Schema(#[from] SchemaMismatch),

    /// A storage operation failed, was refused, or was asked of a reader.
    #[error(transparent)]
    Storage(#[from] StorageError),

    /// The key material could not be created, opened or wrapped.
    #[error(transparent)]
    Keyring(#[from] KeyringError),

    /// A compression level outside the range the compressor defines.
    #[error(transparent)]
    Compression(#[from] InvalidZstdLevel),
}

/// The result of anything this crate can be asked to do.
///
/// Named rather than spelled out at every signature, so that adding a failure mode is one edit here
/// instead of one per function.
pub type Result<T> = std::result::Result<T, Error>;
