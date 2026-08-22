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
use crate::measurement::{Measured, Order, RowId, RowIdSet, intersect_all, union_all, widest_scan};
use crate::search::CandidateSource;
use crate::writer_lock::{LockError, WriteLock};
use crate::{Assignment, Compression, Deleted, Keyring, Predicate, TableDefinition, Updated};
use lance::index::DatasetIndexExt as _;

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
    ) -> impl Future<Output = crate::Result<()>> + Send;

    /// Append a batch to an existing table.
    fn append(
        &self,
        table: &str,
        batch: &RecordBatch,
    ) -> impl Future<Output = crate::Result<()>> + Send;

    /// Read every row of a table back, in the batches it was stored in.
    fn scan(&self, table: &str) -> impl Future<Output = crate::Result<Vec<RecordBatch>>> + Send;

    /// Set columns on every row matching a predicate, and leave every other row alone.
    fn update(
        &self,
        table: &str,
        matching: &Predicate,
        assignments: &[Assignment],
    ) -> impl Future<Output = crate::Result<Updated>> + Send;

    /// Remove a whole table, its rows and its declaration.
    ///
    /// Not the same operation as deleting every row: a dropped name is free again, and a table with no
    /// rows still has a schema.
    fn drop_table(&self, table: &str) -> impl Future<Output = crate::Result<()>> + Send;

    /// Remove every row matching a predicate.
    ///
    /// The count and the predicate are logged **before** anything is removed, so a delete broader than
    /// intended is visible in the log rather than inferred later from missing data.
    fn delete(
        &self,
        table: &str,
        matching: &Predicate,
    ) -> impl Future<Output = crate::Result<Deleted>> + Send;
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
    pub fn open_for_writing(root: impl AsRef<Path>) -> crate::Result<Self> {
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

    /// Search within the rows an exact filter kept, never the other way round.
    ///
    /// The filter runs first and its result is handed to the candidate source, so ranking never considers
    /// a row the caller excluded and never spends work doing so. The order is the point: generating
    /// candidates first and filtering afterwards returns the same rows for a small table and the wrong
    /// ones as soon as a limit truncates the candidates before the filter has had its say.
    ///
    /// # Errors
    ///
    /// Refuses an empty filter, as [`LanceStore::row_ids_all`] does — a search with no filter needs no
    /// pre-filter and should ask the source directly. Otherwise reports the filter's failure, or passes
    /// through the source's own.
    pub async fn filtered_search(
        &self,
        table: &str,
        filter: &[Predicate],
        source: &impl CandidateSource,
        limit: usize,
    ) -> crate::Result<Vec<RowId>> {
        let within = self.row_ids_all(table, filter).await?;
        source.candidates(&within, limit).await
    }

    /// The rows matching **every** one of several predicates.
    ///
    /// Each predicate is answered on its own and the answers are intersected as compressed bitmaps, so a
    /// conjunction costs one narrow read per part rather than one pass evaluating all of them. Where an
    /// index covers a part, that part is served by it.
    ///
    /// # Errors
    ///
    /// Refuses an empty list: an empty conjunction is every row by the identity, and returning a whole
    /// table for a filter a caller believed they had supplied is the kind of surprise that should be an
    /// error. Otherwise reports the failure as [`LanceStore::row_ids`] does.
    pub async fn row_ids_all(
        &self,
        table: &str,
        matching: &[Predicate],
    ) -> crate::Result<RowIdSet> {
        let (first, rest) = self.each_set(table, matching, "intersect").await?;
        Ok(intersect_all(first, rest))
    }

    /// The rows matching **any** one of several predicates.
    ///
    /// # Errors
    ///
    /// Refuses an empty list, for the mirror of the reason [`LanceStore::row_ids_all`] does: an empty
    /// disjunction is no rows at all, and a filter that silently matches nothing hides the mistake
    /// instead of reporting it. Otherwise reports the failure as [`LanceStore::row_ids`] does.
    pub async fn row_ids_any(
        &self,
        table: &str,
        matching: &[Predicate],
    ) -> crate::Result<RowIdSet> {
        let (first, rest) = self.each_set(table, matching, "unite").await?;
        Ok(union_all(first, rest))
    }

    /// One set per predicate, split into the first and the rest.
    ///
    /// Split rather than a list, so the combining functions have no empty case to answer for.
    async fn each_set(
        &self,
        table: &str,
        matching: &[Predicate],
        operation: &'static str,
    ) -> crate::Result<(RowIdSet, Vec<RowIdSet>)> {
        let Some((head, tail)) = matching.split_first() else {
            return Err(StorageError::NothingToCombine {
                table: table.to_owned(),
                operation,
            }
            .into());
        };
        let first: RowIdSet = self.row_ids(table, head).await?.into_iter().collect();
        let mut rest = Vec::with_capacity(tail.len());
        for predicate in tail {
            rest.push(self.row_ids(table, predicate).await?.into_iter().collect());
        }
        Ok((first, rest))
    }

    /// Build an index over a column, so an exact lookup on it stops walking the table.
    ///
    /// **An operation, not a declaration.** The index is built over the rows that are already there,
    /// which is why it cannot be an attribute of a table declared before any row exists.
    ///
    /// Rows appended afterwards stay findable: the index keeps covering the fragments it was built over
    /// and the remainder is scanned, so a lookup examines the newer rows rather than all of them.
    /// Calling this again after a large append rebuilds the index over everything and returns a lookup
    /// to a handful of rows.
    ///
    /// # Errors
    ///
    /// Reports the failure when the table or the column does not exist, and refuses when this store was
    /// opened for reading.
    pub async fn create_index(&self, table: &str, column: &str) -> crate::Result<()> {
        self.require_write_role("index a column of", table)?;
        let mut dataset = self.open(table).await?;
        // No name of ours: the format names an index after the column, and a naming convention we own is
        // one more thing that has to stay stable for no benefit. `replace` so that calling this again
        // after an append rebuilds rather than refusing.
        Box::pin(dataset.create_index(
            &[column],
            lance_index::IndexType::BTree,
            None,
            &lance_index::scalar::ScalarIndexParams::default(),
            true,
        ))
        .await
        .map_err(|cause| StorageError::backend("index a column of", table, cause))?;
        Ok(())
    }

    /// Rows matching a predicate, in the order of one column.
    ///
    /// The predicate is served by an index where one covers the column it constrains, so a range over an
    /// indexed column reads the matching rows rather than the table.
    ///
    /// **The order does not come from the index.** No scalar index in this format returns rows in key
    /// order — both the ordered and the bitmap kind hand back storage order — so the ordering is applied
    /// to the rows the predicate selected. That is a sort over the matches, not over the table, which is
    /// why narrowing first is what keeps it cheap.
    ///
    /// # Errors
    ///
    /// Reports the failure when the table or the ordering column does not exist, or when the predicate
    /// cannot be applied to the table.
    pub async fn scan_ordered(
        &self,
        table: &str,
        matching: &Predicate,
        by: &str,
        order: Order,
    ) -> crate::Result<Vec<RecordBatch>> {
        let dataset = self.open(table).await?;
        let mut scanner = dataset.scan();
        Self::select_ordered(&mut scanner, table, matching, by, order)?;
        self.collect(scanner, table).await
    }

    /// Rows matching a predicate in the order of one column, and how many rows answering it examined.
    ///
    /// **A diagnostic, not a hot path** — as [`LanceStore::row_ids_measured`], it runs the plan a second
    /// time. It exists so the claim that an ordered range still goes through the index is measured
    /// rather than argued: a full scan followed by a sort returns the same rows in the same order.
    ///
    /// # Errors
    ///
    /// As [`LanceStore::scan_ordered`], plus the failure when the plan cannot be analysed.
    pub async fn scan_ordered_measured(
        &self,
        table: &str,
        matching: &Predicate,
        by: &str,
        order: Order,
    ) -> crate::Result<Measured<Vec<RecordBatch>>> {
        let value = self.scan_ordered(table, matching, by, order).await?;

        let dataset = self.open(table).await?;
        let mut scanner = dataset.scan();
        Self::select_ordered(&mut scanner, table, matching, by, order)?;
        let plan = scanner
            .analyze_plan()
            .await
            .map_err(|cause| StorageError::backend("analyse the plan for", table, cause))?;

        Ok(Measured {
            value,
            rows_examined: widest_scan(&plan),
        })
    }

    /// The row ids of every row matching a predicate.
    ///
    /// A row id is the format's identity for a row, so this is what an index points at and what a set
    /// of candidates is built from. Rows are not read: only their identities come back.
    ///
    /// # Errors
    ///
    /// Reports the failure when the table does not exist, or when the predicate cannot be applied to
    /// it — an unknown column, or text that is not an expression.
    pub async fn row_ids(&self, table: &str, matching: &Predicate) -> crate::Result<Vec<RowId>> {
        let dataset = self.open(table).await?;
        let mut scanner = dataset.scan();
        Self::select_row_ids(&mut scanner, table, matching)?;

        let batches = scanner
            .try_into_stream()
            .await
            .map_err(|cause| StorageError::backend("start a scan of the table", table, cause))?
            .try_collect::<Vec<RecordBatch>>()
            .await
            .map_err(|cause| StorageError::backend("read the table", table, cause))?;

        let mut ids = Vec::new();
        for batch in &batches {
            let column = batch.column_by_name(lance_core::ROW_ID).ok_or_else(|| {
                StorageError::backend(
                    "read the row ids of",
                    table,
                    "the scan returned no row-id column",
                )
            })?;
            let raw = column
                .as_any()
                .downcast_ref::<arrow_array::UInt64Array>()
                .ok_or_else(|| {
                    StorageError::backend(
                        "read the row ids of",
                        table,
                        "the row-id column was not the expected type",
                    )
                })?;
            ids.extend((0..batch.num_rows()).map(|row| RowId::new(raw.value(row))));
        }
        Ok(ids)
    }

    /// The row ids of every row matching a predicate, and how many rows answering it examined.
    ///
    /// **A diagnostic, not a hot path.** Measuring means running the plan a second time under the
    /// engine's analysis, so this costs roughly twice what [`LanceStore::row_ids`] costs and exists to
    /// answer one question: did this query use an index, or walk the table?
    ///
    /// # Errors
    ///
    /// As [`LanceStore::row_ids`], plus the failure when the plan cannot be analysed.
    pub async fn row_ids_measured(
        &self,
        table: &str,
        matching: &Predicate,
    ) -> crate::Result<Measured<Vec<RowId>>> {
        let value = self.row_ids(table, matching).await?;

        let dataset = self.open(table).await?;
        let mut scanner = dataset.scan();
        Self::select_row_ids(&mut scanner, table, matching)?;
        let plan = scanner
            .analyze_plan()
            .await
            .map_err(|cause| StorageError::backend("analyse the plan for", table, cause))?;

        Ok(Measured {
            value,
            rows_examined: widest_scan(&plan),
        })
    }

    /// Narrow a scan to the rows matching a predicate, ordered by one column.
    ///
    /// One place, for the same reason as the row-id form: a plan analysed with different options than
    /// the one that ran would measure a different query.
    fn select_ordered(
        scanner: &mut lance::dataset::scanner::Scanner,
        table: &str,
        matching: &Predicate,
        by: &str,
        order: Order,
    ) -> crate::Result<()> {
        scanner
            .filter(matching.as_str())
            .map_err(|cause| StorageError::backend("apply the predicate to", table, cause))?;
        let ordering = match order {
            // Nulls last in both directions: a null is the absence of a key, so it belongs after every
            // row that has one whichever way the keys run.
            Order::Ascending => {
                lance::dataset::scanner::ColumnOrdering::asc_nulls_last(by.to_owned())
            }
            Order::Descending => {
                lance::dataset::scanner::ColumnOrdering::desc_nulls_last(by.to_owned())
            }
        };
        scanner
            .order_by(Some(vec![ordering]))
            .map_err(|cause| StorageError::backend("order the rows of", table, cause))?;
        Ok(())
    }

    /// Read a configured scan into batches carrying the caller's schema.
    ///
    /// One place, because every read has to strip the encoding metadata this crate writes onto a
    /// table's schema — hand back the caller's schema, not the one carrying this crate's encoding
    /// keys — and a read that forgot would hand a caller keys it never declared.
    async fn collect(
        &self,
        scanner: lance::dataset::scanner::Scanner,
        table: &str,
    ) -> crate::Result<Vec<RecordBatch>> {
        scanner
            .try_into_stream()
            .await
            .map_err(|cause| StorageError::backend("start a scan of the table", table, cause))?
            .try_collect::<Vec<RecordBatch>>()
            .await
            .map_err(|cause| StorageError::backend("read the table", table, cause))?
            .into_iter()
            .map(|batch| {
                let schema = Arc::new(Compression::stripped_from(batch.schema_ref()));
                RecordBatch::try_new(schema, batch.columns().to_vec()).map_err(|cause| {
                    crate::Error::from(StorageError::backend("read the table", table, cause))
                })
            })
            .collect()
    }

    /// Narrow a scan to the row ids matching a predicate, and to nothing else.
    ///
    /// One place, because the measured and unmeasured forms have to ask the same question — a plan
    /// analysed with a different projection than the one that ran would measure the wrong query.
    ///
    /// No data columns: the identities are the answer, and reading payload only to discard it would
    /// make every candidate set cost a full row read.
    fn select_row_ids(
        scanner: &mut lance::dataset::scanner::Scanner,
        table: &str,
        matching: &Predicate,
    ) -> crate::Result<()> {
        scanner
            .filter(matching.as_str())
            .map_err(|cause| StorageError::backend("apply the predicate to", table, cause))?;
        scanner.with_row_id();
        scanner
            .project::<&str>(&[])
            .map_err(|cause| StorageError::backend("project no columns of", table, cause))?;
        Ok(())
    }

    /// Refuse a writing operation on a store that was opened for reading.
    fn require_write_role(&self, operation: &'static str, table: &str) -> crate::Result<()> {
        if self.write_lock.is_some() {
            return Ok(());
        }
        Err(StorageError::ReadOnly {
            operation,
            table: table.to_owned(),
        }
        .into())
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
    async fn open(&self, table: &str) -> crate::Result<lance::Dataset> {
        let mut builder =
            lance::dataset::builder::DatasetBuilder::from_uri(self.uri(table).as_str());
        if let Some(session) = self.session.clone() {
            builder = builder.with_session(session);
        }
        builder
            .load()
            .await
            .map_err(|cause| StorageError::backend("open the table", table, cause).into())
    }

    /// The directory holding one table. Tables are separate datasets, so one can be dropped without
    /// rewriting the others.
    fn dataset_path(&self, table: &str) -> PathBuf {
        self.root.join(format!("{table}.lance"))
    }

    /// Where a table lives, as the format's builder wants it.
    fn uri(&self, table: &str) -> String {
        let path = self.dataset_path(table);
        if self.session.is_some() {
            EncryptingProvider::uri(&path)
        } else {
            path.display().to_string()
        }
    }
}

impl TableStore for LanceStore {
    async fn create_table(&self, table: &str, definition: &TableDefinition) -> crate::Result<()> {
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
            .map_err(|cause| StorageError::backend("create the table", table, cause).into())
    }

    async fn append(&self, table: &str, batch: &RecordBatch) -> crate::Result<()> {
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
            .map_err(|cause| StorageError::backend("append to the table", table, cause).into())
    }

    async fn update(
        &self,
        table: &str,
        matching: &Predicate,
        assignments: &[Assignment],
    ) -> crate::Result<Updated> {
        self.require_write_role("update", table)?;
        if assignments.is_empty() {
            return Err(StorageError::NothingToSet {
                table: table.to_owned(),
            }
            .into());
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

    async fn delete(&self, table: &str, matching: &Predicate) -> crate::Result<Deleted> {
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

    async fn drop_table(&self, table: &str) -> crate::Result<()> {
        self.require_write_role("drop the table", table)?;
        // Removing the directory, rather than asking the format to: its own removal for a local
        // dataset is this same synchronous call, and going through the builder would mean opening a
        // table only to delete it.
        std::fs::remove_dir_all(self.dataset_path(table))
            .map_err(|cause| StorageError::backend("drop the table", table, cause).into())
    }

    async fn scan(&self, table: &str) -> crate::Result<Vec<RecordBatch>> {
        let dataset = self.open(table).await?;
        let scanner = dataset.scan();
        self.collect(scanner, table).await
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

    /// Predicates were to be combined, but none were given.
    ///
    /// Its own variant rather than the identity of the operation: an empty conjunction is every row and
    /// an empty disjunction is none, so either default silently answers a question the caller did not
    /// ask.
    #[error("no predicates were given to {operation} for `{table}`")]
    NothingToCombine {
        /// The table the combination was aimed at.
        table: String,
        /// What was to be done with the predicates, phrased to read after "to".
        operation: &'static str,
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
