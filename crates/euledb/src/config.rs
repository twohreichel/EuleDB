//! Every tunable, in one place.

use euledb_storage::Compression;

/// The tunables of a database, with a default for each.
///
/// **What** — one value carrying every knob the database has. **Where** — passed to
/// [`Database::open_with`](crate::Database::open_with) or
/// [`Database::open_for_writing_with`](crate::Database::open_for_writing_with); the plain `open`
/// constructors use [`Config::default`]. **Why one type** — so that a knob is never reachable only by
/// editing source or by an environment variable nobody documented, and so that a tunable added later
/// has an obvious home instead of growing a channel of its own.
///
/// A credential is not a tunable and is not here: a keyring has no sensible default, so encryption is
/// selected when the database is opened, not configured.
///
/// # Examples
///
/// ```
/// use euledb::{Compression, Config, ZstdLevel};
///
/// // The default: zstd at its cheapest level.
/// assert_eq!(Config::default().compression(), Compression::zstd(ZstdLevel::DEFAULT));
///
/// // Trade compression work for space where the data is written once and read often.
/// let archival = Config::default().with_compression(Compression::zstd(ZstdLevel::new(19)?));
/// assert_eq!(archival.compression(), Compression::zstd(ZstdLevel::new(19)?));
/// # Ok::<(), euledb::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    compression: Compression,
}

impl Config {
    /// How table data is compressed on disk.
    ///
    /// **Default** — zstd at its cheapest level. **Effect** — a higher level spends more work per write
    /// for a smaller file, and [`Compression::None`] spends none and stores the encoded bytes as they
    /// are. Reading is unaffected: the level is recorded with the table, so a table written at one
    /// level is read without being told which.
    #[must_use]
    pub fn with_compression(mut self, compression: Compression) -> Self {
        self.compression = compression;
        self
    }

    /// The compression this configuration applies to a table it creates.
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.compression
    }
}
