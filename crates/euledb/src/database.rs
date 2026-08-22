//! The published surface: one handle, and everything a caller does with a table through it.

use std::path::Path;

use arrow_array::RecordBatch;
use euledb_storage::{
    Assignment, Deleted, Embedder, Fused, Keyring, LanceStore, Predicate, RowId, StemmingLanguage,
    TableDefinition, TableSchema, TableStore as _, Updated, VectorIndexKind,
};

use crate::Config;

/// One local database, held open.
///
/// **What** — a directory of tables on this machine, with no server and no network call on the query
/// path. **When** — opened once per process and kept, because opening for writing takes the write role
/// and holds it until the handle is dropped. **Where** — every table lives under the directory the
/// handle was opened on. **Why** a handle rather than free functions: the write role is a property of
/// the handle, so the type system is what stops a reader from writing.
///
/// Many readers may hold the same database at once. At most one writer may, and a second one is
/// refused immediately rather than left waiting.
///
/// The same rows answer three kinds of question — an exact filter, full text, and meaning — and
/// [`Database::hybrid_search`] fuses the last two into one ranking. `docs/getting-started.md` walks
/// through all of them.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use euledb::arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
/// use euledb::arrow_schema::{DataType, Field, Schema};
/// use euledb::{Database, TableSchema};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let temporary = tempfile::tempdir()?;
/// let database = Database::open_for_writing(temporary.path())?;
///
/// let schema = TableSchema::new(Schema::new(vec![
///     Field::new("id", DataType::Int64, false),
///     Field::new("title", DataType::Utf8, false),
/// ]));
/// database.create_table("documents", &schema).await?;
///
/// let id: ArrayRef = Arc::new(Int64Array::from(vec![4218_i64]));
/// let title: ArrayRef = Arc::new(StringArray::from(vec!["Grundsatzurteil"]));
/// let batch = RecordBatch::try_from_iter_with_nullable([
///     ("id", id, false),
///     ("title", title, false),
/// ])?;
/// database.insert("documents", &batch).await?;
///
/// let rows: usize = database.scan("documents").await?.iter().map(RecordBatch::num_rows).sum();
/// assert_eq!(rows, 1);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Database {
    store: LanceStore,
    config: Config,
}

impl Database {
    /// Open a database for reading, taking no write role.
    ///
    /// Any number of readers may hold the same database, alongside a writer.
    #[must_use]
    pub fn open(root: impl AsRef<Path>) -> Self {
        Self::open_with(root, Config::default())
    }

    /// Open a database for reading, with a configuration.
    #[must_use]
    pub fn open_with(root: impl AsRef<Path>, config: Config) -> Self {
        let store = LanceStore::new(root);
        let store = if config.auditing() {
            store.audited()
        } else {
            store
        };
        Self { store, config }
    }

    /// Open a database for writing, taking the write role until the handle is dropped.
    ///
    /// Creates the directory if it does not exist, because a database is opened for writing before it
    /// contains anything.
    ///
    /// # Errors
    ///
    /// Reports the database as already held when another writer has it — immediately, never after a
    /// wait. A local-first database that blocks on a lock held by a process nobody can see is worse
    /// than one that says so.
    pub fn open_for_writing(root: impl AsRef<Path>) -> crate::Result<Self> {
        Self::open_for_writing_with(root, Config::default())
    }

    /// Open a database for writing, with a configuration.
    ///
    /// # Errors
    ///
    /// As [`Database::open_for_writing`].
    pub fn open_for_writing_with(root: impl AsRef<Path>, config: Config) -> crate::Result<Self> {
        let store = LanceStore::open_for_writing(root)?;
        let store = if config.auditing() {
            store.audited()
        } else {
            store
        };
        Ok(Self { store, config })
    }

    /// Read and write this database encrypted under the keyring's data key.
    ///
    /// Every byte of table data is sealed with AES-256-GCM before it reaches the disk. Opening the same
    /// database without the keyring, or with a keyring holding different keys, reports a failure rather
    /// than returning plausible-looking rows.
    #[must_use]
    pub fn encrypted(self, keyring: &Keyring) -> Self {
        Self {
            store: self.store.encrypted(keyring),
            config: self.config,
        }
    }

    /// Give this database the means to turn text into vectors.
    ///
    /// **Required for anything semantic** — inserting into a table with an auto-embedding column, and
    /// every semantic or hybrid query. Not required for exact filters or full text, so a process that
    /// only does those never loads half a gigabyte of weights.
    #[must_use]
    pub fn embedding(self, embedder: std::sync::Arc<dyn Embedder>) -> Self {
        Self {
            store: self.store.embedding(embedder),
            config: self.config,
        }
    }

    /// Build the index a semantic query needs.
    ///
    /// An operation rather than a declaration: the index is built over rows that already exist, so it
    /// cannot be an attribute of a table declared before any row is.
    ///
    /// # Errors
    ///
    /// Reports the failure when the column carries no vectors, or when this handle was opened for reading.
    pub async fn index_vectors(
        &self,
        table: &str,
        column: &str,
        kind: VectorIndexKind,
    ) -> crate::Result<()> {
        self.store.create_vector_index(table, column, kind).await
    }

    /// Build the index a full-text query needs, stemming for one language.
    ///
    /// # Errors
    ///
    /// Reports the failure when the column is not text, or when this handle was opened for reading.
    pub async fn index_text(
        &self,
        table: &str,
        column: &str,
        language: StemmingLanguage,
    ) -> crate::Result<()> {
        self.store.create_text_index(table, column, language).await
    }

    /// The rows whose text is closest in meaning to a query.
    ///
    /// The query is embedded for you, under the prefix the model expects of a *query* rather than of
    /// stored text — a distinction that costs recall when it is got wrong and that a caller should not
    /// have to know about.
    ///
    /// # Errors
    ///
    /// Reports the failure when this database has no embedder, when the column carries no vectors, or
    /// when the search cannot run.
    pub async fn semantic_search(
        &self,
        table: &str,
        column: &str,
        query: &str,
        limit: usize,
    ) -> crate::Result<Vec<RowId>> {
        let vector = self.store.embed_query(query)?;
        Ok(self
            .store
            .nearest(table, column, &vector, limit)
            .await?
            .into_iter()
            .map(|hit| hit.row)
            .collect())
    }

    /// The rows a full-text query matches, ranked by BM25.
    ///
    /// # Errors
    ///
    /// Reports the failure when the column has no text index, or when the query cannot run.
    pub async fn text_search(
        &self,
        table: &str,
        column: &str,
        query: &str,
        limit: usize,
    ) -> crate::Result<Vec<RowId>> {
        self.store.search_text(table, column, query, limit).await
    }

    /// One ranking from both retrieval paths, with the rank each gave every hit.
    ///
    /// # Errors
    ///
    /// Reports the failure when this database has no embedder, when either index is missing, or when
    /// either side cannot answer.
    pub async fn hybrid_search(
        &self,
        table: &str,
        column: &str,
        query: &str,
        limit: usize,
    ) -> crate::Result<Fused> {
        let vector = self.store.embed_query(query)?;
        self.store
            .hybrid_search(table, column, query, &vector, limit)
            .await
    }

    /// Declare a table.
    ///
    /// # Errors
    ///
    /// Reports the failure when the name is already taken or the directory cannot be written.
    pub async fn create_table(&self, table: &str, schema: &TableSchema) -> crate::Result<()> {
        let definition =
            TableDefinition::new(schema.clone()).with_compression(self.config.compression());
        self.store.create_table(table, &definition).await
    }

    /// Append rows to a table.
    ///
    /// The batch must match the table's declared schema, matched by column name rather than position.
    ///
    /// Rows are added, never merged: an insert whose ids already exist stores them a second time. The
    /// layer below calls this `append` for that reason — the name here follows the published API.
    ///
    /// # Errors
    ///
    /// Reports which column was wrong and how when the batch is not the declared table, and reports
    /// the refusal when this handle was opened for reading.
    pub async fn insert(&self, table: &str, rows: &RecordBatch) -> crate::Result<()> {
        self.store.append(table, rows).await
    }

    /// Read every row of a table, in the batches it was stored in.
    ///
    /// # Errors
    ///
    /// Reports the failure when the table does not exist or cannot be read — a missing file, a
    /// permission problem, or key material that does not open it.
    pub async fn scan(&self, table: &str) -> crate::Result<Vec<RecordBatch>> {
        self.store.scan(table).await
    }

    /// Set columns on every row matching a predicate, and leave every other row alone.
    ///
    /// # Errors
    ///
    /// Reports the failure when the predicate or an assignment cannot be applied to the table, when
    /// there is nothing to set, or when this handle was opened for reading.
    pub async fn update(
        &self,
        table: &str,
        matching: &Predicate,
        assignments: &[Assignment],
    ) -> crate::Result<Updated> {
        self.store.update(table, matching, assignments).await
    }

    /// Remove every row matching a predicate, and report how many left.
    ///
    /// The affected count and the predicate are logged at warning level before the rows go, so a delete
    /// wider than intended is readable in the log rather than inferred later from missing data.
    ///
    /// # Errors
    ///
    /// Reports the failure when the predicate cannot be applied to the table, or when this handle was
    /// opened for reading.
    pub async fn delete(&self, table: &str, matching: &Predicate) -> crate::Result<Deleted> {
        self.store.delete(table, matching).await
    }

    /// Remove a whole table, its rows and its declaration.
    ///
    /// Not the same as deleting every row: a dropped name is free again, and a table with no rows still
    /// has a schema.
    ///
    /// # Errors
    ///
    /// Reports the failure when the table is not there, or when this handle was opened for reading.
    pub async fn drop_table(&self, table: &str) -> crate::Result<()> {
        self.store.drop_table(table).await
    }
}
