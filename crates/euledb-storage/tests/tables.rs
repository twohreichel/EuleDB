//! Table lifecycle through the storage port: a table exists, and then it does not.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{Error, LanceStore, StorageError, TableDefinition, TableSchema, TableStore};

/// The shape of a document table, matching the schema tests.
fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
    ])))
}

/// Two real-looking rows, so a count assertion has something to be wrong about.
fn rows() -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(vec![4218_i64, 4219]));
    let title: ArrayRef = Arc::new(StringArray::from(vec![
        "Grundsatzurteil zur Vorratsdatenspeicherung",
        "Rapport sur la souveraineté numérique",
    ]));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("title", title, false)])
        .expect("the batch matches the declared schema")
}

#[tokio::test]
async fn dropping_a_table_removes_it_and_leaves_the_others() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    for table in ["documents", "reports"] {
        store
            .create_table(table, &documents())
            .await
            .expect("the table is declared");
        store.append(table, &rows()).await.expect("the rows land");
    }

    store
        .drop_table("documents")
        .await
        .expect("a table that exists can be dropped");

    assert_refused(
        store.scan("documents").await,
        "documents",
        "reading a table after it was dropped",
    );
    let kept: usize = store
        .scan("reports")
        .await
        .expect("the other table is untouched")
        .iter()
        .map(RecordBatch::num_rows)
        .sum();
    assert_eq!(
        kept, 2,
        "dropping one table must not touch the rows of another",
    );
}

/// Assert that an operation was refused by the layer below, naming the table it was about.
///
/// Not `is_err()`: that assertion survives the wrong error being returned in place of the right one,
/// and `what` keeps the failure line readable when one of several cases breaks.
#[track_caller]
fn assert_refused<T: std::fmt::Debug>(outcome: Result<T, Error>, table: &str, what: &str) {
    match outcome {
        Err(Error::Storage(StorageError::Backend { table: named, .. })) => assert_eq!(
            named.as_str(),
            table,
            "{what}: the error must name the table it was about",
        ),
        other => panic!("{what}: expected a refusal naming `{table}`, got {other:?}"),
    }
}

#[tokio::test]
async fn a_dropped_name_is_free_again() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store
        .append("documents", &rows())
        .await
        .expect("the rows land");
    store.drop_table("documents").await.expect("dropped");

    // Creating the same name again is what tells a hidden table apart from a removed one: a dataset
    // that is merely emptied would still be there, and the second create would fail or inherit rows.
    store
        .create_table("documents", &documents())
        .await
        .expect("the name is free after the drop");
    let rows_now: usize = store
        .scan("documents")
        .await
        .expect("the fresh table reads")
        .iter()
        .map(RecordBatch::num_rows)
        .sum();
    assert_eq!(
        rows_now, 0,
        "a recreated table must not inherit the dropped rows"
    );
}

#[tokio::test]
async fn a_reader_cannot_drop_a_table() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    {
        let writer = LanceStore::open_for_writing(root.path()).expect("the write role is free");
        writer
            .create_table("documents", &documents())
            .await
            .expect("the table is declared");
        writer
            .append("documents", &rows())
            .await
            .expect("the rows land");
    }

    let reader = LanceStore::new(root.path());
    let refusal = reader
        .drop_table("documents")
        .await
        .expect_err("a store opened for reading must not be able to drop a table");
    assert!(
        matches!(
            &refusal,
            Error::Storage(StorageError::ReadOnly { operation, table })
                if *operation == "drop the table" && table == "documents"
        ),
        "the refusal must name what was attempted and on what: {refusal:?}",
    );

    // And the refusal has to be a refusal, not a report: the table is still there afterwards.
    let surviving: usize = reader
        .scan("documents")
        .await
        .expect("the table survived the refused drop")
        .iter()
        .map(RecordBatch::num_rows)
        .sum();
    assert_eq!(surviving, 2, "a refused drop must leave every row in place");
}

#[tokio::test]
async fn dropping_a_table_that_is_not_there_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");

    assert_refused(
        store.drop_table("nie-angelegt").await,
        "nie-angelegt",
        "dropping a table that was never created",
    );
}
