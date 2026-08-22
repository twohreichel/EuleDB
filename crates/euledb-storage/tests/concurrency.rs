//! Multiple readers, one writer, and a second writer told so rather than left waiting.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{LanceStore, StorageError, TableDefinition, TableSchema, TableStore};

fn counters() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![Field::new(
        "id",
        DataType::Int64,
        false,
    )])))
}

fn rows(count: i64) -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from((0..count).collect::<Vec<i64>>()));
    RecordBatch::try_from_iter_with_nullable([("id", id, false)]).expect("batch")
}

#[tokio::test]
async fn a_second_writer_is_refused_while_the_first_holds_the_database() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let first = LanceStore::open_for_writing(root.path()).expect("the first writer must succeed");

    let error = LanceStore::open_for_writing(root.path())
        .expect_err("a second writer must be refused, not admitted and not left waiting");

    assert!(
        matches!(error, StorageError::AlreadyOpenForWriting { .. }),
        "a second writer must be told exactly what is wrong, got: {error:?}",
    );
    drop(first);
}

#[tokio::test]
async fn the_database_can_be_written_again_once_the_writer_is_gone() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    {
        let writer = LanceStore::open_for_writing(root.path()).expect("first writer");
        writer
            .create_table("counters", &counters())
            .await
            .expect("create");
        writer.append("counters", &rows(10)).await.expect("append");
    }

    let second = LanceStore::open_for_writing(root.path())
        .expect("the lock must be released when the writer is dropped");
    second
        .append("counters", &rows(10))
        .await
        .expect("append again");
    let read = second.scan("counters").await.expect("scan");
    assert_eq!(
        read.iter().map(RecordBatch::num_rows).sum::<usize>(),
        20,
        "the second writer did not see or did not add what it should",
    );
}

#[tokio::test]
async fn readers_are_not_blocked_by_a_writer() {
    // The other half of the model: readers are unlimited and never wait. A design that took an exclusive
    // lock for reading would be simpler and would make a local-first database unusable while anything
    // writes to it.
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let writer = LanceStore::open_for_writing(root.path()).expect("writer");
    writer
        .create_table("counters", &counters())
        .await
        .expect("create");
    writer.append("counters", &rows(10)).await.expect("append");

    for reader in 0..3 {
        let rows_read = LanceStore::new(root.path())
            .scan("counters")
            .await
            .unwrap_or_else(|err| panic!("reader {reader} was refused: {err}"))
            .iter()
            .map(RecordBatch::num_rows)
            .sum::<usize>();
        assert_eq!(
            rows_read, 10,
            "reader {reader} saw the wrong number of rows"
        );
    }
    drop(writer);
}

#[tokio::test]
async fn a_reader_refuses_to_write_and_says_so() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    {
        let writer = LanceStore::open_for_writing(root.path()).expect("writer");
        writer
            .create_table("counters", &counters())
            .await
            .expect("create");
    }

    let reader = LanceStore::new(root.path());
    let error = reader
        .append("counters", &rows(10))
        .await
        .expect_err("a store opened for reading must refuse to write");

    assert!(
        matches!(error, StorageError::ReadOnly { .. }),
        "the refusal must name the reason rather than surfacing as something else: {error:?}",
    );
}

#[tokio::test]
async fn two_writers_on_different_databases_do_not_interfere() {
    // The lock is per database, not per process. Two databases open for writing at once is ordinary.
    let first = tempfile::tempdir().expect("a temporary directory is available");
    let second = tempfile::tempdir().expect("a temporary directory is available");

    let one = LanceStore::open_for_writing(first.path()).expect("the first database");
    let two = LanceStore::open_for_writing(second.path())
        .expect("a second database must be writable at the same time");

    one.create_table("counters", &counters())
        .await
        .expect("create in the first");
    two.create_table("counters", &counters())
        .await
        .expect("create in the second");
}
