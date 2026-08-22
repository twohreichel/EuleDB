//! A range predicate answered through the index, with results in key order.

#![recursion_limit = "256"]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{LanceStore, Order, Predicate, TableDefinition, TableSchema, TableStore};

/// A thousand rows, so an indexed range and a full scan differ by two orders of magnitude.
const ROWS: i64 = 1_000;

fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
    ])))
}

/// Rows written in **descending** id, so key order and storage order are different things.
///
/// This is the whole point of the fixture: written ascending, an ordering bug would be invisible
/// because the two orders would coincide.
fn descending() -> RecordBatch {
    let ids: Vec<i64> = (0..ROWS).rev().collect();
    let count = ids.len();
    let id: ArrayRef = Arc::new(Int64Array::from(ids));
    let title: ArrayRef = Arc::new(StringArray::from(
        (0..count)
            .map(|_| "Grundsatzurteil zur Vorratsdatenspeicherung")
            .collect::<Vec<&str>>(),
    ));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("title", title, false)])
        .expect("the batch matches the declared schema")
}

fn ids_of(batches: &[RecordBatch]) -> Vec<i64> {
    batches
        .iter()
        .flat_map(|batch| {
            let column = batch.column_by_name("id").expect("the id column");
            let ids = column
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("ids are Int64");
            (0..batch.num_rows())
                .map(|row| ids.value(row))
                .collect::<Vec<_>>()
        })
        .collect()
}

async fn indexed() -> (tempfile::TempDir, LanceStore) {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store
        .append("documents", &descending())
        .await
        .expect("the rows land");
    store
        .create_index("documents", "id")
        .await
        .expect("an existing column can be indexed");
    (root, store)
}

#[tokio::test]
async fn a_range_comes_back_in_key_order() {
    let (_root, store) = indexed().await;

    let rows = store
        .scan_ordered(
            "documents",
            &Predicate::new("id >= 40 AND id < 50"),
            "id",
            Order::Ascending,
        )
        .await
        .expect("an ordered range over an indexed column resolves");

    // Hand-written, not computed from the query: ten consecutive ids, ascending, from a table that
    // holds them in the opposite order.
    assert_eq!(
        ids_of(&rows),
        vec![40, 41, 42, 43, 44, 45, 46, 47, 48, 49],
        "a range must come back in key order, not in the order the rows were written",
    );
}

#[tokio::test]
async fn a_descending_range_comes_back_largest_first() {
    let (_root, store) = indexed().await;

    let rows = store
        .scan_ordered(
            "documents",
            &Predicate::new("id >= 40 AND id < 50"),
            "id",
            Order::Descending,
        )
        .await
        .expect("an ordered range over an indexed column resolves");

    assert_eq!(
        ids_of(&rows),
        vec![49, 48, 47, 46, 45, 44, 43, 42, 41, 40],
        "descending must run the other way, not merely differ from ascending",
    );
}

/// AC-25 says "through the same index", and that has to be measured rather than argued.
///
/// A full scan followed by a sort returns exactly the same rows in exactly the same order, so the two
/// ordering tests above cannot tell the implementations apart. This one can: it counts the rows the plan
/// examined.
#[tokio::test]
async fn an_ordered_range_still_goes_through_the_index() {
    let (_root, store) = indexed().await;

    let measured = store
        .scan_ordered_measured(
            "documents",
            &Predicate::new("id >= 40 AND id < 50"),
            "id",
            Order::Ascending,
        )
        .await
        .expect("the measured form resolves like the plain one");

    assert_eq!(
        ids_of(&measured.value),
        vec![40, 41, 42, 43, 44, 45, 46, 47, 48, 49],
        "the measured form must return the same rows in the same order",
    );

    let examined = measured.rows_examined.get();
    let total = u64::try_from(ROWS).expect("the corpus fits a u64");
    assert!(
        examined < total / 10,
        "the range must be narrowed by the index before the sort, but {examined} of {total} rows were \
         examined — which is what a full scan followed by a sort looks like",
    );
}

#[tokio::test]
async fn ordering_by_a_column_that_is_not_there_is_refused() {
    let (_root, store) = indexed().await;

    let refusal = store
        .scan_ordered(
            "documents",
            &Predicate::new("id >= 40 AND id < 50"),
            "kein_feld",
            Order::Ascending,
        )
        .await
        .expect_err("a column the table does not have cannot order anything");
    assert!(
        matches!(
            &refusal,
            euledb_storage::Error::Storage(euledb_storage::StorageError::Backend { table, .. })
                if table == "documents"
        ),
        "the refusal must name the table it was about: {refusal:?}",
    );
}

/// An unindexed column still answers correctly, and says what it costs.
///
/// The criterion is about an indexed column, but a caller will order by an unindexed one sooner or
/// later, and the honest behaviour is a correct answer at the price of a scan rather than a refusal.
#[tokio::test]
async fn ordering_by_an_unindexed_column_still_answers_correctly() {
    let (_root, store) = indexed().await;

    let rows = store
        .scan_ordered(
            "documents",
            &Predicate::new("id >= 40 AND id < 43"),
            "title",
            Order::Ascending,
        )
        .await
        .expect("an unindexed ordering column is not a refusal");

    let mut ids = ids_of(&rows);
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![40, 41, 42],
        "the predicate must still select exactly its rows when the ordering column has no index",
    );
}
