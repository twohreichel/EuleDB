//! The storage port, and the on-disk format behind it.
//!
//! Nothing outside this crate names a type from the format. That is the whole point: the format is a
//! pinned dependency chosen for what it saves, and a type leaking out of here would quietly make it
//! permanent (ADR-001).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{RecordBatch, RecordBatchIterator};
use futures_util::TryStreamExt;

use crate::crypto::{BlockSize, Capability, EncryptingProvider, Gate, Scope};
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
    /// Present when this handle can turn text into vectors. Absent means an auto-embedding column is
    /// declared but nothing fills it, which is refused rather than silently skipped.
    embedder: Option<Arc<dyn crate::Embedder>>,
    /// Present when this handle records what it is asked to do.
    audit: Option<crate::audit::AuditLog>,
    /// Present when this handle is gated. Absent means this is the authority's own handle and every
    /// operation is permitted, subject to the write role.
    gate: Option<Gate>,
}

impl LanceStore {
    /// Point a store at a directory. The directory need not exist yet.
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            write_lock: None,
            session: None,
            gate: None,
            audit: None,
            embedder: None,
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
            gate: None,
            audit: None,
            embedder: None,
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
        self.require_scope(Scope::Schema, table)?;
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
        self.require_scope(Scope::Read, table)?;
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
        self.require_scope(Scope::Read, table)?;
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

    /// Give this handle the means to embed text, so an auto-embedding column can fill itself.
    ///
    /// **Why a handle and not a table property** — the model is half a gigabyte of weights and about two
    /// hundred crates. A process that only reads exact filters should not load it, and a table declaring
    /// an embedding column should not force it to.
    ///
    /// Without one, inserting into a table that declares an auto-embedding column is **refused**: a row
    /// stored with no vector is a row no semantic query can ever find, and silence there would be a
    /// database quietly forgetting half of what it was given.
    #[must_use]
    pub fn embedding(mut self, embedder: Arc<dyn crate::Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// The vectors of an auto-embedding column, in row and chunk order.
    ///
    /// # Errors
    ///
    /// Reports the failure when the table has no such auto-embedding column, or when its companion table
    /// cannot be read.
    pub async fn vectors_of(
        &self,
        table: &str,
        column: &str,
    ) -> crate::Result<Vec<crate::RowVector>> {
        self.require_scope(Scope::Read, table)?;
        let batches = self
            .collect(
                self.open(&Self::vector_table(table, column)).await?.scan(),
                table,
            )
            .await?;

        let mut vectors = Vec::new();
        for batch in &batches {
            let rows = batch
                .column_by_name("row")
                .and_then(|column| column.as_any().downcast_ref::<arrow_array::UInt64Array>())
                .ok_or_else(|| {
                    StorageError::backend("read the vectors of", table, "no row column")
                })?;
            let chunks = batch
                .column_by_name("chunk")
                .and_then(|column| column.as_any().downcast_ref::<arrow_array::UInt32Array>())
                .ok_or_else(|| {
                    StorageError::backend("read the vectors of", table, "no chunk column")
                })?;
            let embeddings = batch
                .column_by_name("embedding")
                .and_then(|column| {
                    column
                        .as_any()
                        .downcast_ref::<arrow_array::FixedSizeListArray>()
                })
                .ok_or_else(|| {
                    StorageError::backend("read the vectors of", table, "no embedding column")
                })?;

            for index in 0..batch.num_rows() {
                let values = embeddings.value(index);
                let floats = values
                    .as_any()
                    .downcast_ref::<arrow_array::Float32Array>()
                    .ok_or_else(|| {
                        StorageError::backend(
                            "read the vectors of",
                            table,
                            "embeddings are not f32",
                        )
                    })?;
                vectors.push(crate::RowVector {
                    row: RowId::new(rows.value(index)),
                    chunk: chunks.value(index),
                    embedding: floats.values().to_vec(),
                });
            }
        }
        vectors.sort_by_key(|vector| (vector.row.get(), vector.chunk));
        Ok(vectors)
    }

    /// Build a vector index over an auto-embedding column's vectors.
    ///
    /// HNSW with `m = 16` and cosine distance, over one IVF partition.
    ///
    /// **Two things worth knowing before reading the parameters.** This format's HNSW exists only *inside*
    /// an IVF partitioning — there is no standalone graph — and its build parameters are `max_level`, `m`,
    /// `ef_construction` and a prefetch distance. There is **no separate base-layer connectivity**, so a
    /// distinct value for the bottom layer cannot be set at all.
    ///
    /// One partition, because this is the small-and-mid-size case: IVF over several partitions would push
    /// a query into the wrong one far more often than it would save.
    ///
    /// # Errors
    ///
    /// Reports the failure when the column has no vectors to index, or when the index cannot be built.
    pub async fn create_vector_index(
        &self,
        table: &str,
        column: &str,
        kind: crate::VectorIndexKind,
    ) -> crate::Result<()> {
        self.require_scope(Scope::Schema, table)?;
        self.require_write_role("index the vectors of", table)?;

        let companion = Self::vector_table(table, column);
        let mut dataset = self.open(&companion).await?;

        // One partition: this is the small-and-mid-size case the criterion names, and IVF over a
        // handful of partitions would push a query into the wrong one far more often than it would save.
        let ivf = lance_index::vector::ivf::IvfBuildParams::new(1);
        let params = match kind {
            crate::VectorIndexKind::Graph => {
                // Sixteen connections: the top of the range this project documents. More costs memory
                // and buys recall, and recall is what an index is for.
                let hnsw = lance_index::vector::hnsw::builder::HnswBuildParams {
                    m: 16,
                    ..lance_index::vector::hnsw::builder::HnswBuildParams::default()
                };
                lance::index::vector::VectorIndexParams::with_ivf_hnsw_sq_params(
                    lance_linalg::distance::DistanceType::Cosine,
                    ivf,
                    hnsw,
                    lance_index::vector::sq::builder::SQBuildParams::default(),
                )
            }
            crate::VectorIndexKind::Quantised => {
                // Sixteen sub-vectors of 24 components each, four bits per code. The defaults assume a
                // collection large enough to train 256 centroids per subspace, which the small-and-
                // mid-size case this database targets does not have — a codebook trained on fewer
                // vectors than centroids is not a codebook.
                let pq = lance_index::vector::pq::PQBuildParams {
                    num_sub_vectors: 16,
                    num_bits: 4,
                    ..lance_index::vector::pq::PQBuildParams::default()
                };
                lance::index::vector::VectorIndexParams::with_ivf_pq_params(
                    lance_linalg::distance::DistanceType::Cosine,
                    ivf,
                    pq,
                )
            }
        };

        Box::pin(dataset.create_index(
            &["embedding"],
            lance_index::IndexType::Vector,
            None,
            &params,
            true,
        ))
        .await
        .map_err(|cause| StorageError::backend("index the vectors of", table, cause))?;
        self.record(
            &format!("index the vectors of `{table}`.`{column}`"),
            match kind {
                crate::VectorIndexKind::Graph => "graph",
                crate::VectorIndexKind::Quantised => "quantised",
            },
            0,
        )
    }

    /// The nearest vectors to a query, in order of similarity.
    ///
    /// # Errors
    ///
    /// Reports the failure when the column has no vectors, when the query is the wrong width, or when
    /// the search cannot be run.
    pub async fn nearest(
        &self,
        table: &str,
        column: &str,
        query: &[f32],
        limit: usize,
    ) -> crate::Result<Vec<crate::RowVector>> {
        self.require_scope(Scope::Read, table)?;
        if query.len() != crate::VECTOR_WIDTH {
            return Err(StorageError::WrongVectorWidth {
                given: query.len(),
                wanted: crate::VECTOR_WIDTH,
            }
            .into());
        }

        let companion = Self::vector_table(table, column);
        let dataset = self.open(&companion).await?;
        let mut scanner = dataset.scan();
        let key = arrow_array::Float32Array::from(query.to_vec());
        scanner
            .nearest("embedding", &key, limit)
            .map_err(|cause| StorageError::backend("search the vectors of", table, cause))?;

        let batches = scanner
            .try_into_stream()
            .await
            .map_err(|cause| StorageError::backend("start a search of", table, cause))?
            .try_collect::<Vec<RecordBatch>>()
            .await
            .map_err(|cause| StorageError::backend("read the search results of", table, cause))?;

        let mut found = Vec::new();
        for batch in &batches {
            let rows = batch
                .column_by_name("row")
                .and_then(|c| c.as_any().downcast_ref::<arrow_array::UInt64Array>())
                .ok_or_else(|| {
                    StorageError::backend("read the search results of", table, "no row column")
                })?;
            let chunks = batch
                .column_by_name("chunk")
                .and_then(|c| c.as_any().downcast_ref::<arrow_array::UInt32Array>())
                .ok_or_else(|| {
                    StorageError::backend("read the search results of", table, "no chunk column")
                })?;
            let embeddings = batch
                .column_by_name("embedding")
                .and_then(|c| c.as_any().downcast_ref::<arrow_array::FixedSizeListArray>())
                .ok_or_else(|| {
                    StorageError::backend(
                        "read the search results of",
                        table,
                        "no embedding column",
                    )
                })?;
            for index in 0..batch.num_rows() {
                let values = embeddings.value(index);
                let floats = values
                    .as_any()
                    .downcast_ref::<arrow_array::Float32Array>()
                    .ok_or_else(|| {
                        StorageError::backend("read the search results of", table, "not f32")
                    })?;
                found.push(crate::RowVector {
                    row: RowId::new(rows.value(index)),
                    chunk: chunks.value(index),
                    embedding: floats.values().to_vec(),
                });
            }
        }
        self.record(
            &format!("search the vectors of `{table}`.`{column}`"),
            "",
            u64::try_from(found.len()).unwrap_or(u64::MAX),
        )?;
        Ok(found)
    }

    /// Which vector index a column carries, if any.
    ///
    /// **Why this is public** — "selectable per table" is only a real property if a caller can find out
    /// what was selected. It is also the only reliable way to tell the two kinds apart: the artefacts
    /// differ in size, but two builds of the *same* kind differ in size too, so size proves nothing.
    ///
    /// # Errors
    ///
    /// Reports the failure when the column has no companion table, or when its index metadata cannot be
    /// read.
    pub async fn vector_index_kind(
        &self,
        table: &str,
        column: &str,
    ) -> crate::Result<Option<crate::VectorIndexKind>> {
        self.require_scope(Scope::Read, table)?;
        let companion = Self::vector_table(table, column);
        let dataset = self.open(&companion).await?;

        // The format names an index after its column, and reports its own type in the statistics.
        let statistics = match dataset.index_statistics("embedding_idx").await {
            Ok(statistics) => statistics,
            Err(_) => return Ok(None),
        };
        // A substring test rather than a JSON parse: the only question is which family built it, and
        // pulling in a JSON parser to answer it would be a dependency for one word.
        if statistics.contains("HNSW") {
            Ok(Some(crate::VectorIndexKind::Graph))
        } else if statistics.contains("PQ") {
            Ok(Some(crate::VectorIndexKind::Quantised))
        } else {
            Ok(None)
        }
    }

    /// Whether a nearest-neighbour search would go through the vector index.
    ///
    /// **A diagnostic, and the only way to tell.** With a small collection an exhaustive comparison
    /// returns exactly what the index returns, so no assertion on the *answers* can show that an index
    /// was used at all — a test that only checks the neighbours passes with no index built. This reads
    /// the plan instead.
    ///
    /// # Errors
    ///
    /// As [`LanceStore::nearest`], plus the failure when the plan cannot be explained.
    pub async fn nearest_uses_the_index(
        &self,
        table: &str,
        column: &str,
        query: &[f32],
        limit: usize,
    ) -> crate::Result<bool> {
        self.require_scope(Scope::Read, table)?;
        let companion = Self::vector_table(table, column);
        let dataset = self.open(&companion).await?;
        let mut scanner = dataset.scan();
        let key = arrow_array::Float32Array::from(query.to_vec());
        scanner
            .nearest("embedding", &key, limit)
            .map_err(|cause| StorageError::backend("search the vectors of", table, cause))?;

        let plan = Box::pin(scanner.explain_plan(true))
            .await
            .map_err(|cause| StorageError::backend("explain the search of", table, cause))?;
        // The index contributes its own plan node. A flat comparison of every vector does not.
        Ok(plan.contains("ANNSubIndex") || plan.contains("ANNIvfPartition"))
    }

    /// Bring a column's vectors back into agreement with its text.
    ///
    /// **Reconciliation rather than incremental bookkeeping**, and deliberately so: a row whose text
    /// changed may come back with a different identity, a deleted row leaves its vector behind, and an
    /// inserted row has none. One rule covers all three — vectors whose row is gone are dropped, rows
    /// whose text has no matching vector are embedded — and it is self-healing, so a write interrupted
    /// half way leaves nothing permanently wrong.
    ///
    /// **The cost, stated:** one scan of the table per write. That is the price of correctness here, and
    /// it is the thing to replace when it starts to matter — not the correctness.
    async fn reconcile(&self, table: &str, column: &str) -> crate::Result<()> {
        let Some(embedder) = self.embedder.clone() else {
            return Err(StorageError::backend(
                "embed the auto-embedding column of",
                table,
                "this handle has no embedder: open it with `embedding(..)`",
            )
            .into());
        };

        let companion = Self::vector_table(table, column);
        let existing = match self.open(&companion).await {
            Ok(_) => self.vectors_of(table, column).await?,
            Err(_) => Vec::new(),
        };

        // Row identity and text together, in one pass.
        let dataset = self.open(table).await?;
        let mut scanner = dataset.scan();
        scanner.with_row_id();
        scanner.project(&[column]).map_err(|cause| {
            StorageError::backend("project the embedding column of", table, cause)
        })?;
        let batches = scanner
            .try_into_stream()
            .await
            .map_err(|cause| StorageError::backend("start a scan of the table", table, cause))?
            .try_collect::<Vec<RecordBatch>>()
            .await
            .map_err(|cause| StorageError::backend("read the table", table, cause))?;

        let mut present: Vec<(u64, String)> = Vec::new();
        for batch in &batches {
            let rows = batch
                .column_by_name(lance_core::ROW_ID)
                .and_then(|c| c.as_any().downcast_ref::<arrow_array::UInt64Array>())
                .ok_or_else(|| {
                    StorageError::backend("read the row ids of", table, "no row-id column")
                })?;
            let texts = batch
                .column_by_name(column)
                .and_then(|c| c.as_any().downcast_ref::<arrow_array::StringArray>())
                .ok_or_else(|| {
                    StorageError::backend(
                        "read the embedding column of",
                        table,
                        "an auto-embedding column must hold text",
                    )
                })?;
            for index in 0..batch.num_rows() {
                present.push((rows.value(index), texts.value(index).to_owned()));
            }
        }

        let live: std::collections::BTreeSet<u64> = present.iter().map(|(row, _)| *row).collect();
        let vectored: std::collections::BTreeSet<u64> =
            existing.iter().map(|vector| vector.row.get()).collect();

        // Nothing to do is the common case after a write that touched no embedding column.
        if live == vectored {
            return Ok(());
        }

        // Rewritten wholesale rather than patched. A companion table is derived data: rebuilding it is
        // always correct, and reasoning about a partial patch after an interrupted write is not.
        let mut rows = Vec::new();
        let mut chunks = Vec::new();
        let mut values: Vec<f32> = Vec::new();
        for (row, text) in &present {
            let kept: Vec<Vec<f32>> = existing
                .iter()
                .filter(|vector| vector.row.get() == *row)
                .map(|vector| vector.embedding.clone())
                .collect();
            let embeddings = if kept.is_empty() {
                embedder
                    .embed_passage(text)
                    .map_err(|cause| StorageError::backend("embed the text of", table, cause))?
            } else {
                kept
            };
            for (chunk, embedding) in embeddings.into_iter().enumerate() {
                if embedding.len() != crate::VECTOR_WIDTH {
                    return Err(StorageError::backend(
                        "embed the text of",
                        table,
                        format!(
                            "the embedder produced {} components, not {}",
                            embedding.len(),
                            crate::VECTOR_WIDTH
                        ),
                    )
                    .into());
                }
                rows.push(*row);
                chunks.push(u32::try_from(chunk).unwrap_or(u32::MAX));
                values.extend(embedding);
            }
        }

        let schema = Arc::new(Self::vector_schema());
        let embeddings = arrow_array::FixedSizeListArray::try_new(
            Arc::new(arrow_schema::Field::new(
                "item",
                arrow_schema::DataType::Float32,
                false,
            )),
            i32::try_from(crate::VECTOR_WIDTH).unwrap_or(i32::MAX),
            Arc::new(arrow_array::Float32Array::from(values)),
            None,
        )
        .map_err(|cause| StorageError::backend("build the vectors of", table, cause))?;
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(arrow_array::UInt64Array::from(rows)),
                Arc::new(arrow_array::UInt32Array::from(chunks)),
                Arc::new(embeddings),
            ],
        )
        .map_err(|cause| StorageError::backend("build the vectors of", table, cause))?;

        let params = lance::dataset::WriteParams {
            mode: lance::dataset::WriteMode::Overwrite,
            ..Default::default()
        };
        let iterator = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema);
        lance::Dataset::write(iterator, self.uri(&companion).as_str(), Some(params))
            .await
            .map_err(|cause| StorageError::backend("write the vectors of", table, cause))?;
        Ok(())
    }

    /// Reconcile every auto-embedding column of a table, or do nothing if it declares none.
    ///
    /// Called after every write rather than by the caller, which is the whole content of the criterion:
    /// a vector that has to be refreshed by hand is a vector that will not be.
    async fn reconcile_all(&self, table: &str) -> crate::Result<()> {
        for column in self.embedding_columns(table).await? {
            self.reconcile(table, &column).await?;
        }
        Ok(())
    }

    /// The columns of a table that embed themselves, read from the table's own schema.
    async fn embedding_columns(&self, table: &str) -> crate::Result<Vec<String>> {
        let dataset = self.open(table).await?;
        let schema = arrow_schema::Schema::from(dataset.schema());
        Ok(crate::TableSchema::new(schema).auto_embedding_columns())
    }

    /// The companion table holding one column's vectors.
    ///
    /// A separate table rather than a column on the same row, because chunking means one row owns an
    /// ordered *set* of vectors and a row cannot hold a variable number of fixed-width lists usefully.
    fn vector_table(table: &str, column: &str) -> String {
        format!("{table}.{column}.vectors")
    }

    /// The schema of a companion table.
    fn vector_schema() -> arrow_schema::Schema {
        arrow_schema::Schema::new(vec![
            arrow_schema::Field::new("row", arrow_schema::DataType::UInt64, false),
            arrow_schema::Field::new("chunk", arrow_schema::DataType::UInt32, false),
            arrow_schema::Field::new(
                "embedding",
                arrow_schema::DataType::FixedSizeList(
                    Arc::new(arrow_schema::Field::new(
                        "item",
                        arrow_schema::DataType::Float32,
                        false,
                    )),
                    i32::try_from(crate::VECTOR_WIDTH).unwrap_or(i32::MAX),
                ),
                false,
            ),
        ])
    }

    /// Record every operation this handle performs in the database's audit log.
    ///
    /// **Why a tunable rather than always on** — a recorded read is a *write*, so an audited handle
    /// cannot open a database on read-only media or one the caller may only read. Off means no file is
    /// created at all.
    ///
    /// **Why a read is recorded** — the criterion says every operation, and who read what is usually the
    /// question an audit log is opened to answer.
    ///
    /// The log takes a short exclusive lock on **its own file**, never the database's write lock, so
    /// many readers still hold the database at once and serialise only for the length of one append.
    #[must_use]
    pub fn audited(mut self) -> Self {
        self.audit = Some(crate::audit::AuditLog::open(&self.root));
        self
    }

    /// Record one operation, if this handle records anything.
    ///
    /// A failing log fails the operation. The alternative — carrying on with a gap in the record — is
    /// worse than refusing: a log that is silently incomplete still looks trustworthy.
    ///
    /// # Errors
    ///
    /// Whatever the log could not do.
    fn record(&self, query: &str, plan: &str, rows: u64) -> crate::Result<()> {
        match &self.audit {
            None => Ok(()),
            Some(log) => log.append(query, plan, rows).map_err(Into::into),
        }
    }

    /// Restrict this handle to what the given capabilities permit.
    ///
    /// **What** — turns the authority's own handle into one that may do only what a signed token says.
    /// **When** — before handing a handle to a subsystem or a plugin that should not have the whole
    /// database. **Why the authority is not gated by default** — the holder of the keyring *is* the
    /// authority, and a database with no restricted handle in it has nobody to restrict.
    ///
    /// Inside a gated handle the default is **nothing**: every operation needs a token naming its table
    /// and its scope, so read-only is what an empty grant list means. Scopes do not imply one another —
    /// a write token does not permit reading.
    ///
    /// A token is honoured only if its tag verifies under the keyring that signed it, so a holder cannot
    /// widen its own rights by writing a capability by hand.
    #[must_use]
    pub fn gated(mut self, keyring: &Keyring, granted: Vec<Capability>) -> Self {
        self.gate = Some(Gate::new(keyring, granted));
        self
    }

    /// Refuse an operation this handle has no token for.
    ///
    /// Checked **before the table is touched**, which is what keeps the refusal from revealing whether
    /// the target exists: a caller without permission gets the same answer for a table that is there and
    /// one that is not.
    fn require_scope(&self, scope: Scope, table: &str) -> crate::Result<()> {
        match &self.gate {
            None => Ok(()),
            Some(gate) if gate.permits(table, scope) => Ok(()),
            Some(_) => Err(StorageError::NotPermitted {
                scope,
                table: table.to_owned(),
            }
            .into()),
        }
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
        self.require_scope(Scope::Schema, table)?;
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
            .map_err(|cause| StorageError::backend("create the table", table, cause))?;
        self.record(&format!("create table `{table}`"), "", 0)
    }

    async fn append(&self, table: &str, batch: &RecordBatch) -> crate::Result<()> {
        self.require_scope(Scope::Write, table)?;
        self.require_write_role("append to", table)?;
        let schema = batch.schema();
        let batches = RecordBatchIterator::new(std::iter::once(Ok(batch.clone())), schema);
        let params = lance::dataset::WriteParams {
            mode: lance::dataset::WriteMode::Append,
            session: self.session.clone(),
            ..Default::default()
        };
        let appended = u64::try_from(batch.num_rows()).unwrap_or(u64::MAX);
        lance::Dataset::write(batches, self.uri(table).as_str(), Some(params))
            .await
            .map_err(|cause| StorageError::backend("append to the table", table, cause))?;
        self.record(&format!("insert into `{table}`"), "", appended)?;
        self.reconcile_all(table).await
    }

    async fn update(
        &self,
        table: &str,
        matching: &Predicate,
        assignments: &[Assignment],
    ) -> crate::Result<Updated> {
        self.require_scope(Scope::Write, table)?;
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
        self.record(
            &format!("update `{table}` where {}", matching.as_str()),
            &assignments
                .iter()
                .map(|assignment| assignment.column())
                .collect::<Vec<&str>>()
                .join(", "),
            result.rows_updated,
        )?;
        self.reconcile_all(table).await?;
        Ok(Updated {
            rows: result.rows_updated,
        })
    }

    async fn delete(&self, table: &str, matching: &Predicate) -> crate::Result<Deleted> {
        self.require_scope(Scope::Write, table)?;
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
        self.record(
            &format!("delete from `{table}` where {}", matching.as_str()),
            "",
            result.num_deleted_rows,
        )?;
        self.reconcile_all(table).await?;
        Ok(Deleted {
            rows: result.num_deleted_rows,
        })
    }

    async fn drop_table(&self, table: &str) -> crate::Result<()> {
        self.require_scope(Scope::Schema, table)?;
        self.require_write_role("drop the table", table)?;
        self.record(&format!("drop table `{table}`"), "", 0)?;
        // Removing the directory, rather than asking the format to: its own removal for a local
        // dataset is this same synchronous call, and going through the builder would mean opening a
        // table only to delete it.
        std::fs::remove_dir_all(self.dataset_path(table))
            .map_err(|cause| StorageError::backend("drop the table", table, cause).into())
    }

    async fn scan(&self, table: &str) -> crate::Result<Vec<RecordBatch>> {
        self.require_scope(Scope::Read, table)?;
        let dataset = self.open(table).await?;
        let scanner = dataset.scan();
        let batches = self.collect(scanner, table).await?;
        let returned = batches
            .iter()
            .map(|batch| u64::try_from(batch.num_rows()).unwrap_or(u64::MAX))
            .sum();
        self.record(&format!("scan `{table}`"), "", returned)?;
        Ok(batches)
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

    /// A query vector is not the width the stored vectors are.
    ///
    /// Its own variant rather than a backend failure: nothing below was asked anything. A vector of the
    /// wrong length cannot be compared to anything, and saying so with both numbers is the difference
    /// between a usable message and "could not search".
    #[error("a query vector of {given} components cannot be compared to vectors of {wanted}")]
    WrongVectorWidth {
        /// What the caller supplied.
        given: usize,
        /// What the stored vectors are.
        wanted: usize,
    },

    /// The handle has no token for what was asked.
    ///
    /// Deliberately identical whether or not the table exists: the check runs before the table is
    /// touched, so a caller without permission cannot use the error to discover what a database holds.
    #[error("this handle has no {scope} capability for `{table}`")]
    NotPermitted {
        /// The scope that was missing.
        scope: Scope,
        /// The table that was asked for — the name the caller supplied, which tells them nothing new.
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
