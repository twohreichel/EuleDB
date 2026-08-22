//! The declared shape of a table, and the gate every insert passes through.

use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_schema::{DataType, Schema, SchemaRef};

/// The declared shape of a table.
///
/// A table's schema is an Apache Arrow schema, so the same description serves storage, the query path
/// and anything the caller already uses Arrow for — there is no second, private notion of a type.
/// Constructing one does not touch a disk.
#[derive(Debug, Clone)]
pub struct TableSchema {
    declared: SchemaRef,
}

impl TableSchema {
    /// Declare a table's shape.
    ///
    /// ```
    /// use arrow_schema::{DataType, Field, Schema};
    /// use euledb_storage::TableSchema;
    ///
    /// let schema = TableSchema::new(Schema::new(vec![
    ///     Field::new("id", DataType::Int64, false),
    ///     Field::new("body", DataType::Utf8, true),
    /// ]));
    /// assert_eq!(schema.declared().fields().len(), 2);
    /// ```
    #[must_use]
    pub fn new(schema: Schema) -> Self {
        Self {
            declared: Arc::new(schema),
        }
    }

    /// The Arrow schema this table was declared with.
    #[must_use]
    pub fn declared(&self) -> &SchemaRef {
        &self.declared
    }

    /// Check a batch against the declaration before anything is written.
    ///
    /// Columns are matched by name rather than by position, and a batch may be *stricter* than the
    /// declaration — forbidding null where the declaration permits it is not a mismatch.
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
    /// use arrow_schema::{DataType, Field, Schema};
    /// use euledb_storage::{Error, SchemaMismatch, TableSchema};
    ///
    /// let schema = TableSchema::new(Schema::new(vec![
    ///     Field::new("id", DataType::Int64, false),
    ///     Field::new("title", DataType::Utf8, false),
    /// ]));
    ///
    /// // Identifiers read out of JSON arrive as strings, which is the mistake worth catching early.
    /// let id: ArrayRef = Arc::new(StringArray::from(vec!["4218"]));
    /// let title: ArrayRef = Arc::new(StringArray::from(vec!["Grundsatzurteil"]));
    /// let batch = RecordBatch::try_from_iter_with_nullable([
    ///     ("id", id, false),
    ///     ("title", title, false),
    /// ])?;
    ///
    /// let error = schema.validate(&batch).unwrap_err();
    /// assert!(matches!(error, Error::Schema(SchemaMismatch::TypeMismatch { .. })));
    /// assert_eq!(
    ///     error.to_string(),
    ///     "column `id` is declared Int64 but the batch carries Utf8",
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`SchemaMismatch`] when the batch does not match the declared schema, naming the column
    /// and what was wrong with it.
    pub fn validate(&self, batch: &RecordBatch) -> crate::Result<()> {
        let present = batch.schema();
        // Matched by name rather than by position: a caller assembling columns from a map has no
        // control over their order, and refusing a correct batch for that would be pedantry.
        for declared in self.declared.fields() {
            let Some((_, found)) = present.column_with_name(declared.name()) else {
                return Err(SchemaMismatch::MissingColumn {
                    column: declared.name().clone(),
                }
                .into());
            };
            if found.data_type() != declared.data_type() {
                return Err(SchemaMismatch::TypeMismatch {
                    column: declared.name().clone(),
                    declared: declared.data_type().clone(),
                    present: found.data_type().clone(),
                }
                .into());
            }
            // A batch may be stricter than the declaration but never looser. Declaring a column
            // non-nullable and then storing nulls in it makes the declaration a suggestion.
            if found.is_nullable() && !declared.is_nullable() {
                return Err(SchemaMismatch::NullabilityMismatch {
                    column: declared.name().clone(),
                    declared_nullable: false,
                }
                .into());
            }
        }
        for field in present.fields() {
            if self.declared.column_with_name(field.name()).is_none() {
                return Err(SchemaMismatch::UndeclaredColumn {
                    column: field.name().clone(),
                }
                .into());
            }
        }
        Ok(())
    }
}

/// Why a batch was refused.
///
/// Every variant names the column it is about. An insert rejected without saying which column was
/// wrong leaves the caller to bisect their own data, which is the failure this type exists to prevent.
#[derive(Debug, thiserror::Error)]
pub enum SchemaMismatch {
    /// A column the schema declares is absent from the batch.
    #[error("the batch has no column `{column}`, which the schema declares")]
    MissingColumn {
        /// The declared column that is absent.
        column: String,
    },

    /// A column the batch carries that the schema does not declare.
    ///
    /// Refused rather than dropped. Silently discarding a column the caller believed they were storing
    /// is how data goes missing without anyone finding out until it is needed.
    #[error("the batch carries column `{column}`, which the schema does not declare")]
    UndeclaredColumn {
        /// The column that is not part of the declaration.
        column: String,
    },

    /// A column's type is not the declared one.
    #[error("column `{column}` is declared {declared} but the batch carries {present}")]
    TypeMismatch {
        /// The column whose type differs.
        column: String,
        /// The type the schema declares.
        declared: DataType,
        /// The type the batch actually carries.
        present: DataType,
    },

    /// The batch permits null in a column the declaration does not.
    ///
    /// The reverse is allowed: a batch may be stricter than the declaration.
    #[error("column `{column}` is declared non-nullable, but the batch permits null in it")]
    NullabilityMismatch {
        /// The column whose nullability differs.
        column: String,
        /// What the schema declares. Always `false` today, because the looser direction is accepted.
        declared_nullable: bool,
    },
}
