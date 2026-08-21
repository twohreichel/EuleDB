//! Insert validation against a declared table schema, through the public interface.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{SchemaMismatch, TableSchema};

/// One column of a batch: its name, its data, and whether it permits null.
type Column = (&'static str, ArrayRef, bool);

/// The shape of a document table, which is what this database is actually for: an identifier, text to
/// search, and the language and publication date a query filters on.
fn documents() -> TableSchema {
    TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("body", DataType::Utf8, true),
        Field::new("language", DataType::Utf8, false),
    ]))
}

/// One row of a real-looking document, matching `documents()` exactly.
///
/// Each test starts from this and changes the one thing it is about, so the body of a test shows the
/// deviation rather than four lines of identical setup.
fn document_columns() -> Vec<Column> {
    vec![
        ("id", Arc::new(Int64Array::from(vec![4218])), false),
        (
            "title",
            Arc::new(StringArray::from(vec![
                "Grundsatzurteil zur Vorratsdatenspeicherung",
            ])),
            false,
        ),
        (
            "body",
            Arc::new(StringArray::from(vec![Some(
                "Der Erste Senat hat entschieden …",
            )])),
            true,
        ),
        ("language", Arc::new(StringArray::from(vec!["de"])), false),
    ]
}

/// Assemble columns into a batch. A batch is buildable from any consistent set of columns — whether it
/// matches a declared schema is exactly the question under test.
fn batch(columns: Vec<Column>) -> RecordBatch {
    RecordBatch::try_from_iter_with_nullable(columns)
        .expect("columns of equal length always form a batch")
}

/// Position of a column in `document_columns`, so a test can replace one without a magic index.
fn index_of(columns: &[Column], name: &str) -> usize {
    columns
        .iter()
        .position(|(column, _, _)| *column == name)
        .unwrap_or_else(|| panic!("`{name}` is one of the document columns"))
}

#[test]
fn accepts_a_batch_whose_schema_matches_the_declaration() {
    documents()
        .validate(&batch(document_columns()))
        .expect("a batch built from the declared schema must be accepted");
}

#[test]
fn rejects_a_batch_missing_a_declared_column_and_names_it() {
    let mut columns = document_columns();
    columns.remove(index_of(&columns, "language"));

    let error = documents()
        .validate(&batch(columns))
        .expect_err("a batch missing a declared column must be refused");

    assert!(
        matches!(&error, SchemaMismatch::MissingColumn { column } if column == "language"),
        "expected a missing-column error naming `language`, got: {error:?}",
    );
    assert!(
        error.to_string().contains("language"),
        "the message must name the offending column, and this one does not: {error}",
    );
}

#[test]
fn rejects_a_batch_with_an_undeclared_column_and_names_it() {
    let mut columns = document_columns();
    // A caller who computed embeddings and expects the table to store them. Dropping the column
    // silently is how data goes missing without anyone finding out until it is needed.
    columns.push((
        "embedding",
        Arc::new(StringArray::from(vec!["[0.13, -0.02, …]"])),
        true,
    ));

    let error = documents()
        .validate(&batch(columns))
        .expect_err("a column the schema does not declare must be refused, not silently dropped");

    assert!(
        matches!(&error, SchemaMismatch::UndeclaredColumn { column } if column == "embedding"),
        "expected an undeclared-column error naming `embedding`, got: {error:?}",
    );
    assert!(
        error.to_string().contains("embedding"),
        "the message must name the offending column: {error}",
    );
}

#[test]
fn rejects_a_column_of_the_wrong_type_and_names_both_types() {
    let mut columns = document_columns();
    // `id` is declared Int64. A caller reading identifiers out of JSON or CSV hands over strings
    // without noticing, which is exactly the mistake worth catching before it reaches the disk.
    let position = index_of(&columns, "id");
    columns[position] = ("id", Arc::new(StringArray::from(vec!["4218"])), false);

    let error = documents()
        .validate(&batch(columns))
        .expect_err("a column whose type differs from the declaration must be refused");

    assert!(
        matches!(
            &error,
            SchemaMismatch::TypeMismatch { column, declared, present }
                if column == "id" && *declared == DataType::Int64 && *present == DataType::Utf8
        ),
        "expected a type mismatch on `id` from Int64 to Utf8, got: {error:?}",
    );
    let message = error.to_string();
    for expected in ["id", "Int64", "Utf8"] {
        assert!(
            message.contains(expected),
            "the message must name the column and both types, but {expected:?} is missing: {message}",
        );
    }
}

#[test]
fn rejects_a_nullable_column_where_the_declaration_forbids_null() {
    let mut columns = document_columns();
    // `title` is declared non-nullable. A batch that permits null there is not the declared table, and
    // accepting it would make the declaration a suggestion.
    let position = index_of(&columns, "title");
    columns[position].2 = true;

    let error = documents()
        .validate(&batch(columns))
        .expect_err("a batch that permits null in a non-nullable column must be refused");

    assert!(
        matches!(
            &error,
            SchemaMismatch::NullabilityMismatch { column, declared_nullable: false }
                if column == "title"
        ),
        "expected a nullability mismatch on `title`, got: {error:?}",
    );
}

#[test]
fn accepts_a_batch_stricter_than_the_declaration() {
    let mut columns = document_columns();
    // `body` is declared nullable. A batch that forbids null there is stricter, not wrong: every value
    // it carries satisfies the declaration.
    let position = index_of(&columns, "body");
    columns[position].2 = false;

    documents()
        .validate(&batch(columns))
        .expect("a batch stricter than the declaration still satisfies it");
}
