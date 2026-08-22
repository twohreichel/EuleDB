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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    compression: Compression,
    auditing: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            compression: Compression::default(),
            auditing: true,
        }
    }
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

    /// Whether every operation is recorded in the database's hash-chained audit log.
    ///
    /// **Default** — on. **Effect** — each operation appends one record naming what was asked and how
    /// many rows it affected, chained so that a removed or altered entry does not go unnoticed. Reads
    /// are operations: who read what is usually the question an audit log is opened to answer.
    ///
    /// **The consequence of switching it off, and of leaving it on.** A recorded read is a *write*, so
    /// an audited handle cannot open a database on read-only media or one the caller may only read —
    /// turn this off there, and accept that reads then leave no trace. Left on, the log grows by one
    /// line per operation and is never pruned by this database.
    #[must_use]
    pub fn with_auditing(mut self, auditing: bool) -> Self {
        self.auditing = auditing;
        self
    }

    /// Whether operations are recorded.
    #[must_use]
    pub const fn auditing(&self) -> bool {
        self.auditing
    }
}
