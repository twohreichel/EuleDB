//! Row identity, and how many rows an operation had to look at.

// Computing the layout of the format's plan-analysis future costs 130 levels of query depth, and the
// default ceiling is 128. It only bites on Linux with a compiler newer than the pinned one — macOS and
// Windows stable compile it either way — so the matrix over two toolchains AND four platforms is what
// surfaced it. Raising the ceiling is the compiler's own remedy; boxing the future at the call site was
// tried first and only moved the cost inside the dependency, where it still has to be paid.
#![recursion_limit = "256"]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{
    LanceStore, Measured, Predicate, RowId, TableDefinition, TableSchema, TableStore,
};

/// Enough rows that a full scan and a narrow one are different numbers by an order of magnitude.
const ROWS: i64 = 1_000;

fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
    ])))
}

fn rows() -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from((0..ROWS).collect::<Vec<i64>>()));
    let title: ArrayRef = Arc::new(StringArray::from(
        (0..ROWS)
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
    store.append("documents", &rows()).await.expect("rows land");
    (root, store)
}

#[tokio::test]
async fn row_ids_identify_the_rows_a_predicate_matches() {
    let (_root, store) = populated().await;

    let matching = store
        .row_ids("documents", &Predicate::new("id = 42"))
        .await
        .expect("a predicate over an existing column resolves");
    assert_eq!(
        matching.len(),
        1,
        "one row carries id 42, so one row id must come back",
    );

    // The identity has to be stable and usable: asking again yields the same id, and a wider
    // predicate contains it. An id that changed between calls could not index anything.
    let again = store
        .row_ids("documents", &Predicate::new("id = 42"))
        .await
        .expect("the same query resolves again");
    assert_eq!(again, matching, "a row id must not change between reads");

    let wider = store
        .row_ids("documents", &Predicate::new("id >= 40 AND id < 50"))
        .await
        .expect("a range predicate resolves");
    assert_eq!(wider.len(), 10, "ten rows fall in the range");
    assert!(
        wider.contains(&matching[0]),
        "the narrow match must be one of the wider match's rows",
    );
}

#[tokio::test]
async fn row_ids_of_a_predicate_matching_nothing_are_empty() {
    let (_root, store) = populated().await;

    let none = store
        .row_ids("documents", &Predicate::new("id = -1"))
        .await
        .expect("a predicate matching nothing is not an error");
    assert!(
        none.is_empty(),
        "a predicate that matches no row must yield no row id, not a failure",
    );
}

#[tokio::test]
async fn an_unindexed_lookup_examines_the_whole_table() {
    let (_root, store) = populated().await;

    let measured: Measured<Vec<RowId>> = store
        .row_ids_measured("documents", &Predicate::new("id = 42"))
        .await
        .expect("the measured form resolves like the plain one");

    assert_eq!(
        measured.value.len(),
        1,
        "the result must be the same result"
    );
    // No index exists yet, so the only way to answer this is to look at every row. That is the
    // baseline the next ticket has to beat, and asserting it now is what makes the improvement
    // visible rather than claimed.
    let examined = measured.rows_examined.get();
    let corpus = u64::try_from(ROWS).expect("the corpus fits a u64");
    // Bounded on both sides on purpose. The lower bound is the claim — no index means every row. The
    // upper bound is what stops a fabricated number from passing: a measurement that simply returned
    // something enormous would satisfy "at least the whole table" and prove nothing.
    assert!(
        (corpus..corpus * 2).contains(&examined),
        "without an index the widest step must examine about {corpus} rows, but it reported {examined}",
    );
}

#[tokio::test]
async fn row_ids_within_a_table_are_distinct() {
    let (_root, store) = populated().await;

    let ascending = store
        .row_ids("documents", &Predicate::new("id >= 0 AND id < 5"))
        .await
        .expect("a range predicate resolves");

    // Row ids are the format's identity for a row, not our key order. They are comparable so a set
    // can be built from them, and that is all this asserts — reading key order out of them is what
    // the index ticket is for.
    let mut sorted = ascending.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        ascending.len(),
        "row ids within one table must be distinct",
    );
}
