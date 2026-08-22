//! Persistence through the storage port: what goes in comes back out, unchanged, after a reopen.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{LanceStore, TableDefinition, TableSchema, TableStore};

/// The shape of a document table, matching the schema tests.
fn documents() -> TableSchema {
    TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("language", DataType::Utf8, false),
    ]))
}

/// Three real-looking rows, enough that an ordering or off-by-one bug has somewhere to hide.
fn rows() -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(vec![4218, 4219, 4220]));
    let title: ArrayRef = Arc::new(StringArray::from(vec![
        "Grundsatzurteil zur Vorratsdatenspeicherung",
        "Rapport sur la souveraineté numérique",
        "Ustawa o ochronie danych osobowych",
    ]));
    let language: ArrayRef = Arc::new(StringArray::from(vec!["de", "fr", "pl"]));
    RecordBatch::try_from_iter_with_nullable([
        ("id", id, false),
        ("title", title, false),
        ("language", language, false),
    ])
    .expect("the batch matches the declared schema")
}

#[tokio::test]
async fn rows_survive_a_drop_and_reopen_unchanged() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let written = rows();

    {
        let store =
            LanceStore::open_for_writing(root.path()).expect("taking the write role must succeed");
        store
            .create_table("documents", &TableDefinition::new(documents()))
            .await
            .expect("creating a table in an empty directory must succeed");
        store
            .append("documents", &written)
            .await
            .expect("a batch matching the schema must be accepted");
    } // the handle goes out of scope here — the point of the test

    let reopened = LanceStore::new(root.path());
    let read_back = reopened
        .scan("documents")
        .await
        .expect("a table written and closed must be readable again");

    assert_eq!(
        read_back,
        vec![written],
        "the rows that came back are not the rows that went in",
    );
}

/// One further row, so a second append has something distinguishable to add.
fn later_row() -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(vec![4221]));
    let title: ArrayRef = Arc::new(StringArray::from(vec![
        "Besluit inzake gegevensbescherming",
    ]));
    let language: ArrayRef = Arc::new(StringArray::from(vec!["nl"]));
    RecordBatch::try_from_iter_with_nullable([
        ("id", id, false),
        ("title", title, false),
        ("language", language, false),
    ])
    .expect("the batch matches the declared schema")
}

#[tokio::test]
async fn a_second_append_adds_to_the_first_instead_of_replacing_it() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store =
        LanceStore::open_for_writing(root.path()).expect("taking the write role must succeed");
    store
        .create_table("documents", &TableDefinition::new(documents()))
        .await
        .expect("creating a table in an empty directory must succeed");

    store
        .append("documents", &rows())
        .await
        .expect("first append");
    store
        .append("documents", &later_row())
        .await
        .expect("second append");

    let read_back = store
        .scan("documents")
        .await
        .expect("the table is readable");
    let total: usize = read_back.iter().map(RecordBatch::num_rows).sum();

    assert_eq!(
        total, 4,
        "three rows then one more must leave four. Got {total}, so an append replaced what was \
         already there instead of adding to it",
    );
}
