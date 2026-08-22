//! An indexed column answers an exact lookup without walking the table.

#![recursion_limit = "256"]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{LanceStore, Predicate, TableDefinition, TableSchema, TableStore};

/// A thousand rows: a full scan and an indexed lookup differ by three orders of magnitude, and the
/// suite still runs in well under a second.
const ROWS: i64 = 1_000;

fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
    ])))
}

fn batch(ids: std::ops::Range<i64>) -> RecordBatch {
    let count = usize::try_from(ids.end - ids.start).expect("the range fits a usize");
    let id: ArrayRef = Arc::new(Int64Array::from(ids.collect::<Vec<i64>>()));
    let title: ArrayRef = Arc::new(StringArray::from(
        (0..count)
            .map(|_| "Grundsatzurteil zur Vorratsdatenspeicherung")
            .collect::<Vec<&str>>(),
    ));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("title", title, false)])
        .expect("the batch matches the declared schema")
}

async fn populated() -> (tempfile::TempDir, LanceStore) {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store
        .append("documents", &batch(0..ROWS))
        .await
        .expect("the rows land");
    (root, store)
}

#[tokio::test]
async fn an_indexed_lookup_examines_a_handful_of_rows() {
    let (_root, store) = populated().await;
    store
        .create_index("documents", "id")
        .await
        .expect("an existing column can be indexed");

    let measured = store
        .row_ids_measured("documents", &Predicate::new("id = 42"))
        .await
        .expect("the lookup resolves");

    // The answer must still be the right answer. An index that is fast and wrong is worse than a scan.
    assert_eq!(
        measured.value.len(),
        1,
        "exactly one row carries id 42, index or no index",
    );

    // The claim of AC-24, and the reason SUB-19 measured the baseline first: without an index this same
    // lookup examined all 1000 rows. A handful means the index answered it. The bound is generous on
    // purpose — the point is the order of magnitude, not a precise count the engine renders for humans.
    let examined = measured.rows_examined.get();
    assert!(
        examined < 10,
        "an indexed lookup must examine a handful of rows, not {examined} of {ROWS}",
    );
}

/// The surprising half of how this index behaves, and the reason it is pinned by a test.
///
/// An index covers the rows it was built over. Rows appended afterwards are still found — correctly —
/// but by scanning the part the index does not cover. So a lookup after a big append examines the
/// newer rows, not the whole table and not a handful either, and rebuilding brings it back down.
#[tokio::test]
async fn rows_appended_after_the_index_are_found_by_scanning_only_the_remainder() {
    /// Small next to the indexed thousand, so "scanned the remainder" and "scanned everything" are
    /// different numbers rather than the same one.
    const LATER: i64 = 100;

    let (_root, store) = populated().await;
    store
        .create_index("documents", "id")
        .await
        .expect("an existing column can be indexed");
    store
        .append("documents", &batch(ROWS..ROWS + LATER))
        .await
        .expect("later rows land");

    let new_row = store
        .row_ids_measured("documents", &Predicate::new("id = 1050"))
        .await
        .expect("the lookup resolves");
    assert_eq!(
        new_row.value.len(),
        1,
        "a row appended after the index must still be findable",
    );

    let examined = new_row.rows_examined.get();
    let later = u64::try_from(LATER).expect("the corpus fits a u64");
    let total = u64::try_from(ROWS + LATER).expect("the corpus fits a u64");
    assert!(
        (examined <= later * 2) && examined < total,
        "only the part the index does not cover should be examined, but {examined} of {total} were",
    );

    // Rebuilding covers everything again, which is what makes the append cost recoverable rather than
    // permanent. Without `replace` this call would refuse and the number would stay where it was.
    store
        .create_index("documents", "id")
        .await
        .expect("an index can be rebuilt over the rows added since");
    let rebuilt = store
        .row_ids_measured("documents", &Predicate::new("id = 1050"))
        .await
        .expect("the lookup resolves")
        .rows_examined
        .get();
    assert!(
        rebuilt < 10,
        "after a rebuild the lookup must be back to a handful of rows, not {rebuilt}",
    );
}

#[tokio::test]
async fn indexing_a_column_that_is_not_there_is_refused() {
    let (_root, store) = populated().await;

    let refusal = store
        .create_index("documents", "kein_feld")
        .await
        .expect_err("a column the table does not have cannot be indexed");
    assert!(
        matches!(
            &refusal,
            euledb_storage::Error::Storage(euledb_storage::StorageError::Backend { table, .. })
                if table == "documents"
        ),
        "the refusal must name the table it was about: {refusal:?}",
    );
}

#[tokio::test]
async fn a_reader_cannot_create_an_index() {
    let (root, store) = populated().await;
    drop(store);

    let reader = LanceStore::new(root.path());
    let refusal = reader
        .create_index("documents", "id")
        .await
        .expect_err("a store opened for reading must not be able to build an index");
    assert!(
        matches!(
            &refusal,
            euledb_storage::Error::Storage(euledb_storage::StorageError::ReadOnly { operation, table })
                if *operation == "index a column of" && table == "documents"
        ),
        "the refusal must name what was attempted and on what: {refusal:?}",
    );
}

/// The index serves a range, not only an equality.
///
/// This is not AC-25 — that criterion is about the *order* results come back in, and it belongs to the
/// ticket that implements ordered ranges. This one only claims that a range is answered through the
/// index rather than by walking the table.
///
/// It does **not** defend the choice of index kind, which is what it was written to do: a mutation
/// swapping the ordered index for the bitmap one survives it, because in this format a bitmap index
/// serves a range without a full scan too. The choice rests on cardinality instead — a bitmap over a
/// thousand distinct integers is a thousand bitmaps — and it becomes behaviour a test can see only when
/// ordering is exercised.
#[tokio::test]
async fn the_index_serves_a_range_without_walking_the_table() {
    let (_root, store) = populated().await;
    store
        .create_index("documents", "id")
        .await
        .expect("an existing column can be indexed");

    let measured = store
        .row_ids_measured("documents", &Predicate::new("id >= 40 AND id < 50"))
        .await
        .expect("a range predicate resolves");

    assert_eq!(
        measured.value.len(),
        10,
        "ten rows fall in the half-open range",
    );
    let examined = measured.rows_examined.get();
    let total = u64::try_from(ROWS).expect("the corpus fits a u64");
    assert!(
        examined < total / 10,
        "a range must be served by the index, but {examined} of {total} rows were examined",
    );
}
