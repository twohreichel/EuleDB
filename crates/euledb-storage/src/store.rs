//! The storage port, and the on-disk format behind it.
//!
//! Nothing outside this crate names a type from the format. That is the whole point: the format is a
//! pinned dependency chosen for what it saves, and a type leaking out of here would quietly make it
//! permanent (ADR-001).

use std::path::{Path, PathBuf};

use arrow_array::{RecordBatch, RecordBatchIterator};
use futures_util::TryStreamExt;

use crate::TableSchema;

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
        schema: &TableSchema,
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
}

/// A store holding each table as a dataset under one directory.
///
/// Constructing one touches no disk and creates nothing — a store is a location, not a handle, so it
/// can be created and dropped freely. Reopening the same path sees what was written before.
#[derive(Debug, Clone)]
pub struct LanceStore {
    root: PathBuf,
}

impl LanceStore {
    /// Point a store at a directory. The directory need not exist yet.
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Where a table lives. Tables are separate datasets, so one can be dropped without rewriting
    /// the others.
    fn uri(&self, table: &str) -> String {
        self.root
            .join(format!("{table}.lance"))
            .display()
            .to_string()
    }
}

impl TableStore for LanceStore {
    async fn create_table(&self, table: &str, schema: &TableSchema) -> Result<(), StorageError> {
        // An empty batch iterator carrying the schema. The declaration is what is being persisted
        // here — a table with no rows still has a shape, and that shape is what a later append is
        // checked against.
        let empty = RecordBatchIterator::new(std::iter::empty(), schema.declared().clone());
        let params = lance::dataset::WriteParams {
            mode: lance::dataset::WriteMode::Create,
            ..Default::default()
        };
        lance::Dataset::write(empty, self.uri(table).as_str(), Some(params))
            .await
            .map(|_| ())
            .map_err(|cause| StorageError::backend("create the table", table, cause))
    }

    async fn append(&self, table: &str, batch: &RecordBatch) -> Result<(), StorageError> {
        let schema = batch.schema();
        let batches = RecordBatchIterator::new(std::iter::once(Ok(batch.clone())), schema);
        let params = lance::dataset::WriteParams {
            mode: lance::dataset::WriteMode::Append,
            ..Default::default()
        };
        lance::Dataset::write(batches, self.uri(table).as_str(), Some(params))
            .await
            .map(|_| ())
            .map_err(|cause| StorageError::backend("append to the table", table, cause))
    }

    async fn scan(&self, table: &str) -> Result<Vec<RecordBatch>, StorageError> {
        let dataset = lance::Dataset::open(self.uri(table).as_str())
            .await
            .map_err(|cause| StorageError::backend("open the table", table, cause))?;
        dataset
            .scan()
            .try_into_stream()
            .await
            .map_err(|cause| StorageError::backend("start a scan of the table", table, cause))?
            .try_collect()
            .await
            .map_err(|cause| StorageError::backend("read the table", table, cause))
    }
}

/// A storage operation failed.
///
/// The operation and the table are always named, because "storage error" without them tells an operator
/// nothing they can act on.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
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
