//! The storage port, and the on-disk format behind it.
//!
//! Nothing outside this crate names a type from the format. That is the whole point: the format is a
//! pinned dependency chosen for what it saves, and a type leaking out of here would quietly make it
//! permanent (ADR-001).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{RecordBatch, RecordBatchIterator};
use futures_util::TryStreamExt;

use crate::crypto::{BlockSize, EncryptingProvider};
use crate::writer_lock::{LockError, WriteLock};
use crate::{Assignment, Compression, Deleted, Keyring, Predicate, TableDefinition, Updated};

/// A cause from the layer below, kept as an opaque source.
///
/// Typed deliberately as a boxed error rather than the backend's own error type: naming that type here
/// would put it in this crate's public API, and then every caller would depend on the format that is
/// supposed to be replaceable.
type Cause = Box<dyn std::error::Error + Send + Sync>;

/// Somewhere tables can be created, appended to and read back.
///
/// A port rather than an abstraction over variants: it exists to invert the dependency on the on-disk
/// format, so one implementation is the expected number, not a sign of a missing second one.
pub trait TableStore {
    /// Declare a table. Fails if it already exists.
    fn create_table(
        &self,
        table: &str,
        definition: &TableDefinition,
    ) -> impl Future<Output = Result<(), StorageError>> + Send;

    /// Append a batch to an existing table.
    fn append(
        &self,
        table: &str,
        batch: &RecordBatch,
    ) -> impl Future<Output = Result<(), StorageError>> + Send;

    /// Read every row of a table back, in the batches it was stored in.
    fn scan(
        &self,
        table: &str,
    ) -> impl Future<Output = Result<Vec<RecordBatch>, StorageError>> + Send;

    /// Set columns on every row matching a predicate, and leave every other row alone.
    fn update(
        &self,
        table: &str,
        matching: &Predicate,
        assignments: &[Assignment],
    ) -> impl Future<Output = Result<Updated, StorageError>> + Send;

    /// Remove every row matching a predicate.
    ///
    /// The count and the predicate are logged **before** anything is removed, so a delete broader than
    /// intended is visible in the log rather than inferred later from missing data.
    fn delete(
        &self,
        table: &str,
        matching: &Predicate,
    ) -> impl Future<Output = Result<Deleted, StorageError>> + Send;
}

/// A store holding each table as a dataset under one directory.
///
/// # The concurrency model
///
/// **Any number of readers, at most one writer, per database directory.**
///
/// - [`LanceStore::new`] opens for reading. It touches no disk, takes no lock, and never waits. Readers
///   are unlimited and a writer does not block them.
/// - [`LanceStore::open_for_writing`] takes the write role and holds it until the store is dropped. A
///   second writer is **refused immediately** with [`StorageError::AlreadyOpenForWriting`] rather than
///   queued: a local-first database that blocks forever on a lock held by a process nobody can see is
///   worse than one that says so.
/// - Calling a writing method on a reader is refused with [`StorageError::ReadOnly`].
///
/// The lock is advisory and lives with an open file handle, so it is released when the writer is dropped
/// **and** when its process dies for any reason, including being killed. A marker file would outlive a
/// crash and lock the database out permanently.
#[derive(Debug, Clone)]
pub struct LanceStore {
    root: PathBuf,
    /// Present only on a store opened for writing. Holding it IS the write role.
    write_lock: Option<Arc<WriteLock>>,
    /// Present when this store is encrypted. Carries the registry that resolves the encrypted URI
    /// scheme, which is how every byte gets routed through the cipher.
    session: Option<Arc<lance::session::Session>>,
}

impl LanceStore {
    /// Point a store at a directory. The directory need not exist yet.
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            write_lock: None,
            session: None,
        }
    }

    /// Open the database for writing, taking the write role.
    ///
    /// Creates the directory if it does not exist, because a database is opened for writing before it
    /// contains anything. The role is held until this store is dropped.
    ///
    /// # Errors
    ///
    /// [`StorageError::AlreadyOpenForWriting`] when another writer holds it, immediately rather than
    /// after a wait. [`StorageError::Backend`] when the lock cannot be established at all, which is a
    /// filesystem or permission problem rather than contention.
    pub fn open_for_writing(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        let root = root.as_ref().to_path_buf();
        let lock = WriteLock::acquire(&root).map_err(|cause| match cause {
            LockError::Busy { root } => StorageError::AlreadyOpenForWriting { root },
            other => StorageError::backend(
                "take the write lock for",
                &root.display().to_string(),
                other,
            ),
        })?;
        Ok(Self {
            root,
            write_lock: Some(Arc::new(lock)),
            session: None,
        })
    }

    /// Refuse a writing operation on a store that was opened for reading.
    fn require_write_role(&self, operation: &'static str, table: &str) -> Result<(), StorageError> {
        if self.write_lock.is_some() {
            return Ok(());
        }
        Err(StorageError::ReadOnly {
            operation,
            table: table.to_owned(),
        })
    }

    /// Read and write this store's tables encrypted under the keyring's data key.
    ///
    /// Addresses the tables under a private URI scheme rather than as plain files, and that is the
    /// mechanism rather than decoration: the format writes local files through a path that bypasses its
    /// own object-store abstraction entirely, so only a scheme it does not recognise as local routes the
    /// bytes through the cipher. `docs/adr/ADR-002-where-encryption-sits.md` § Amendment carries the
    /// evidence and the cost.
    ///
    /// The block size is fixed at the default: it is part of the on-disk layout, so a per-table choice
    /// would have to be persisted where a reader sees it before it can read anything.
    #[must_use]
    pub fn encrypted(mut self, keyring: &Keyring) -> Self {
        let registry = EncryptingProvider::registry(keyring.frame(BlockSize::default()));
        // Its own session, because the registry it carries holds this database's cipher. A shared one
        // would hand one database's key to another.
        self.session = Some(Arc::new(lance::session::Session::new(
            lance::dataset::DEFAULT_INDEX_CACHE_SIZE,
            lance::dataset::DEFAULT_METADATA_CACHE_SIZE,
            registry,
        )));
        self
    }

    /// Open a table.
    ///
    /// The builder rather than `Dataset::open`, because open uses the process-wide registry and would
    /// not know the encrypted scheme. One place, so that reading, updating and deleting cannot drift
    /// into opening the same table differently.
    async fn open(&self, table: &str) -> Result<lance::Dataset, StorageError> {
        let mut builder =
            lance::dataset::builder::DatasetBuilder::from_uri(self.uri(table).as_str());
        if let Some(session) = self.session.clone() {
            builder = builder.with_session(session);
        }
        builder
            .load()
            .await
            .map_err(|cause| StorageError::backend("open the table", table, cause))
    }

    /// Where a table lives. Tables are separate datasets, so one can be dropped without rewriting
    /// the others.
    fn uri(&self, table: &str) -> String {
        let path = self.root.join(format!("{table}.lance"));
        if self.session.is_some() {
            EncryptingProvider::uri(&path)
        } else {
            path.display().to_string()
        }
    }
}

impl TableStore for LanceStore {
    async fn create_table(
        &self,
        table: &str,
        definition: &TableDefinition,
    ) -> Result<(), StorageError> {
        self.require_write_role("create the table", table)?;
        // The compression travels as field metadata on the schema, so it is persisted with the table
        // rather than having to be supplied again on every write.
        let encoded = definition
            .compression()
            .applied_to(definition.schema().declared());
        // An empty batch iterator carrying the schema. The declaration is what is being persisted
        // here — a table with no rows still has a shape, and that shape is what a later append is
        // checked against.
        let empty = RecordBatchIterator::new(std::iter::empty(), Arc::new(encoded));
        let params = lance::dataset::WriteParams {
            mode: lance::dataset::WriteMode::Create,
            session: self.session.clone(),
            ..Default::default()
        };
        lance::Dataset::write(empty, self.uri(table).as_str(), Some(params))
            .await
            .map(|_| ())
            .map_err(|cause| StorageError::backend("create the table", table, cause))
    }

    async fn append(&self, table: &str, batch: &RecordBatch) -> Result<(), StorageError> {
        self.require_write_role("append to", table)?;
        let schema = batch.schema();
        let batches = RecordBatchIterator::new(std::iter::once(Ok(batch.clone())), schema);
        let params = lance::dataset::WriteParams {
            mode: lance::dataset::WriteMode::Append,
            session: self.session.clone(),
            ..Default::default()
        };
        lance::Dataset::write(batches, self.uri(table).as_str(), Some(params))
            .await
            .map(|_| ())
            .map_err(|cause| StorageError::backend("append to the table", table, cause))
    }

    async fn update(
        &self,
        table: &str,
        matching: &Predicate,
        assignments: &[Assignment],
    ) -> Result<Updated, StorageError> {
        self.require_write_role("update", table)?;
        if assignments.is_empty() {
            return Err(StorageError::NothingToSet {
                table: table.to_owned(),
            });
        }
        let dataset = Arc::new(self.open(table).await?);
        let mut builder = lance::dataset::write::update::UpdateBuilder::new(dataset)
            .update_where(matching.as_str())
            .map_err(|cause| StorageError::backend("apply the predicate to", table, cause))?;
        for assignment in assignments {
            builder = builder
                .set(assignment.column(), assignment.value())
                .map_err(|cause| StorageError::backend("apply an assignment to", table, cause))?;
        }
        let job = builder
            .build()
            .map_err(|cause| StorageError::backend("plan the update of", table, cause))?;
        let result = job
            .execute()
            .await
            .map_err(|cause| StorageError::backend("update", table, cause))?;
        Ok(Updated {
            rows: result.rows_updated,
        })
    }

    async fn delete(&self, table: &str, matching: &Predicate) -> Result<Deleted, StorageError> {
        self.require_write_role("delete from", table)?;
        let mut dataset = self.open(table).await?;

        // Counted first, and logged before the delete runs. That ordering is the requirement rather
        // than a convenience: a delete wider than intended has to be visible in the log at the moment
        // it is about to happen, not deduced afterwards from rows that are no longer there.
        let announced = u64::try_from(
            dataset
                .count_rows(Some(matching.as_str().to_owned()))
                .await
                .map_err(|cause| {
                    StorageError::backend("count the rows to delete from", table, cause)
                })?,
        )
        .unwrap_or(u64::MAX);
        // WARNING rather than INFO: removing rows degrades what the database holds, and an operator who
        // reads only warnings still has to see it.
        tracing::warn!(
            table,
            predicate = matching.as_str(),
            rows = announced,
            "deleting rows",
        );

        let result = dataset
            .delete(matching.as_str())
            .await
            .map_err(|cause| StorageError::backend("delete from", table, cause))?;
        Ok(Deleted {
            rows: result.num_deleted_rows,
        })
    }

    async fn scan(&self, table: &str) -> Result<Vec<RecordBatch>, StorageError> {
        let dataset = self.open(table).await?;
        dataset
            .scan()
            .try_into_stream()
            .await
            .map_err(|cause| StorageError::backend("start a scan of the table", table, cause))?
            .try_collect::<Vec<RecordBatch>>()
            .await
            .map_err(|cause| StorageError::backend("read the table", table, cause))?
            .into_iter()
            .map(|batch| {
                // Hand back the caller's schema, not the one carrying this crate's encoding keys.
                let schema = Arc::new(Compression::stripped_from(batch.schema_ref()));
                RecordBatch::try_new(schema, batch.columns().to_vec())
                    .map_err(|cause| StorageError::backend("read the table", table, cause))
            })
            .collect()
    }
}

/// A storage operation failed.
///
/// The operation and the table are always named, because "storage error" without them tells an operator
/// nothing they can act on.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Another writer already holds this database.
    #[error("another writer already holds the database at {}", root.display())]
    AlreadyOpenForWriting {
        /// The database that is held.
        root: PathBuf,
    },

    /// A writing operation was asked of a store that was opened for reading.
    #[error("cannot {operation} `{table}`: this database was opened for reading")]
    ReadOnly {
        /// What was attempted, phrased to read after "cannot".
        operation: &'static str,
        /// The table it was aimed at.
        table: String,
    },

    /// An update was asked for with no columns to set.
    ///
    /// Its own variant rather than a silent no-op: an update that changes nothing is a mistake at the
    /// call site, and reporting success would hide it.
    #[error("an update of `{table}` was asked for with nothing to set")]
    NothingToSet {
        /// The table the update was aimed at.
        table: String,
    },

    /// The layer below refused or could not complete the operation.
    #[error("could not {operation} `{table}`")]
    Backend {
        /// What was being attempted, phrased to read after "could not".
        operation: &'static str,
        /// The table the operation was about.
        table: String,
        /// The underlying failure.
        ///
        /// Deliberately opaque: naming the backend's own error type here would put it in this
        /// crate's public API, and then every caller would depend on the format that is supposed
        /// to stay replaceable. The chain is still reachable through
        /// [`Error::source`](std::error::Error::source).
        #[source]
        cause: Cause,
    },
}

impl StorageError {
    /// Wrap a failure from the layer below, naming what was being attempted and on what.
    fn backend(operation: &'static str, table: &str, cause: impl Into<Cause>) -> Self {
        Self::Backend {
            operation,
            table: table.to_owned(),
            cause: cause.into(),
        }
    }
}
