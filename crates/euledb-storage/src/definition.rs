//! Everything a table needs at creation time.

use crate::{Compression, TableSchema};

/// A table, as declared before it exists.
///
/// A parameter bundle rather than a widening signature: the settings a table is created with only grow,
/// and each one is fixed for the table's life, so they belong together in one value the caller builds
/// and reads back.
#[derive(Debug, Clone)]
pub struct TableDefinition {
    schema: TableSchema,
    compression: Compression,
}

impl TableDefinition {
    /// Declare a table with the default compression.
    ///
    /// ```
    /// use arrow_schema::{DataType, Field, Schema};
    /// use euledb_storage::{Compression, TableDefinition, TableSchema, ZstdLevel};
    ///
    /// let schema = TableSchema::new(Schema::new(vec![
    ///     Field::new("body", DataType::Utf8, false),
    /// ]));
    ///
    /// // The default is zstd at the cheapest level, which measured within 2 % of the smallest.
    /// let table = TableDefinition::new(schema.clone());
    /// assert_eq!(table.compression(), Compression::zstd(ZstdLevel::DEFAULT));
    ///
    /// // Trade compression work for space where the data is written once and read often.
    /// let archival = TableDefinition::new(schema).with_compression(
    ///     Compression::zstd(ZstdLevel::new(19)?),
    /// );
    /// assert_eq!(archival.compression(), Compression::zstd(ZstdLevel::new(19)?));
    /// # Ok::<(), euledb_storage::InvalidZstdLevel>(())
    /// ```
    #[must_use]
    pub fn new(schema: TableSchema) -> Self {
        Self {
            schema,
            compression: Compression::default(),
        }
    }

    /// Choose the compression for this table.
    #[must_use]
    pub fn with_compression(mut self, compression: Compression) -> Self {
        self.compression = compression;
        self
    }

    /// The declared shape.
    #[must_use]
    pub fn schema(&self) -> &TableSchema {
        &self.schema
    }

    /// The compression this table will be written with.
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.compression
    }
}
