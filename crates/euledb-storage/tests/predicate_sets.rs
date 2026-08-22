//! Conjunctive and disjunctive predicates, combined as sets of row ids.

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
    Error, LanceStore, Predicate, StorageError, TableDefinition, TableSchema, TableStore,
};

/// A thousand rows with ids 0..1000 — the corpus every expected count below is read off by hand.
const ROWS: i64 = 1_000;

fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("language", DataType::Utf8, false),
    ])))
}

/// Ids ascending, languages cycling through three values, so a predicate on each selects a known share.
fn rows() -> RecordBatch {
    let languages = ["de", "fr", "pl"];
    let id: ArrayRef = Arc::new(Int64Array::from((0..ROWS).collect::<Vec<i64>>()));
    let language: ArrayRef = Arc::new(StringArray::from(
        (0..ROWS)
            .map(|row| languages[usize::try_from(row).expect("fits") % languages.len()])
            .collect::<Vec<&str>>(),
    ));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("language", language, false)])
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
async fn a_conjunction_is_the_intersection_of_its_parts() {
    let (_root, store) = populated().await;

    let combined = store
        .row_ids_all(
            "documents",
            &[
                Predicate::new("id >= 40 AND id < 70"),
                Predicate::new("language = 'de'"),
            ],
        )
        .await
        .expect("a conjunction over existing columns resolves");

    // Hand-read off the corpus: ids 40..70 is thirty rows, every third row is `de`, and 42 is the first
    // multiple of three in that window — 42, 45, ... 69 is ten rows.
    assert_eq!(
        combined.len(),
        10,
        "thirty rows in the range, every third of them German, is ten",
    );

    // And the same question asked as one expression must give the same answer. A different evaluation
    // path — the engine's own conjunction rather than two result sets intersected — so agreement is
    // evidence rather than a tautology.
    let brute_force = store
        .row_ids(
            "documents",
            &Predicate::new("id >= 40 AND id < 70 AND language = 'de'"),
        )
        .await
        .expect("the equivalent single predicate resolves");
    let mut expected: Vec<u64> = brute_force.iter().map(|id| id.get()).collect();
    expected.sort_unstable();
    let mut got: Vec<u64> = combined.iter().map(|id| id.get()).collect();
    got.sort_unstable();
    assert_eq!(
        got, expected,
        "intersecting two sets must select exactly what one combined filter selects",
    );
}

#[tokio::test]
async fn a_disjunction_is_the_union_of_its_parts() {
    let (_root, store) = populated().await;

    let combined = store
        .row_ids_any(
            "documents",
            &[Predicate::new("id < 5"), Predicate::new("id >= 995")],
        )
        .await
        .expect("a disjunction over existing columns resolves");

    // Hand-read: five rows at each end of a thousand, and the two ends do not overlap.
    assert_eq!(combined.len(), 10, "five plus five, with nothing in common");

    let brute_force = store
        .row_ids("documents", &Predicate::new("id < 5 OR id >= 995"))
        .await
        .expect("the equivalent single predicate resolves");
    let mut expected: Vec<u64> = brute_force.iter().map(|id| id.get()).collect();
    expected.sort_unstable();
    assert_eq!(
        combined.iter().map(|id| id.get()).collect::<Vec<u64>>(),
        expected,
        "uniting two sets must select exactly what one combined filter selects",
    );
}

/// The test that tells the two operators apart.
///
/// Every other assertion in this file would still pass if `all` united and `any` intersected, as long as
/// it did so consistently. These two counts differ, and they differ by a lot.
#[tokio::test]
async fn the_same_parts_combined_the_two_ways_give_different_answers() {
    let (_root, store) = populated().await;
    let parts = [
        Predicate::new("id < 100"),
        Predicate::new("language = 'de'"),
    ];

    let both = store
        .row_ids_all("documents", &parts)
        .await
        .expect("a conjunction resolves");
    let either = store
        .row_ids_any("documents", &parts)
        .await
        .expect("a disjunction resolves");

    // Hand-read off the corpus: languages cycle de, fr, pl from row 0, so `de` is every third row —
    // 334 of a thousand, and 34 of the first hundred. The union is 100 + 334 - 34.
    assert_eq!(
        both.len(),
        34,
        "every third of the first hundred rows is German"
    );
    assert_eq!(
        either.len(),
        400,
        "a hundred, plus 334 German, less the 34 counted twice"
    );
    assert!(
        both.len() < either.len(),
        "a conjunction cannot select more rows than the disjunction of the same parts",
    );

    // Every row of the conjunction is in the disjunction. The relation holds for any inputs, which is
    // what makes it worth asserting alongside the two counts.
    for row in both.iter() {
        assert!(
            either.contains(row),
            "row {row:?} is in both parts, so it must be in either",
        );
    }
}

#[tokio::test]
async fn one_predicate_combines_to_itself() {
    let (_root, store) = populated().await;
    let only = Predicate::new("id >= 40 AND id < 50");

    let alone = store
        .row_ids("documents", &only)
        .await
        .expect("the predicate resolves");
    let mut expected: Vec<u64> = alone.iter().map(|id| id.get()).collect();
    expected.sort_unstable();

    let one = std::slice::from_ref(&only);
    for combined in [
        store.row_ids_all("documents", one).await,
        store.row_ids_any("documents", one).await,
    ] {
        let set = combined.expect("a single-part combination resolves");
        assert_eq!(
            set.iter().map(|id| id.get()).collect::<Vec<u64>>(),
            expected,
            "combining one predicate with nothing must not change what it selects",
        );
    }
}

#[tokio::test]
async fn combining_no_predicates_at_all_is_refused() {
    let (_root, store) = populated().await;

    // An empty conjunction is every row by the identity and an empty disjunction is none, so either
    // default answers a question the caller did not ask. Both refuse, and the refusal names which
    // operation was attempted so the message is usable.
    for (outcome, expected_operation) in [
        (store.row_ids_all("documents", &[]).await, "intersect"),
        (store.row_ids_any("documents", &[]).await, "unite"),
    ] {
        let refusal = outcome.expect_err("an empty predicate list must be refused");
        assert!(
            matches!(
                &refusal,
                Error::Storage(StorageError::NothingToCombine { table, operation })
                    if table == "documents" && *operation == expected_operation
            ),
            "the refusal must name the table and the operation: {refusal:?}",
        );
    }
}
