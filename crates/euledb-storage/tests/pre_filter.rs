//! An exact filter narrows the rows before candidate generation ever sees them.

#![recursion_limit = "256"]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;
use std::sync::Mutex;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{
    CandidateSource, LanceStore, Predicate, RowId, RowIdSet, TableDefinition, TableSchema,
    TableStore,
};

/// A thousand rows, so "narrowed first" and "the whole table" are different numbers.
const ROWS: i64 = 1_000;

fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("language", DataType::Utf8, false),
    ])))
}

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

/// A hand-written fake, not a mock: it records the set it was handed and answers from it.
///
/// A mock asserting that a call happened would prove nothing about the order of operations, which is
/// the whole content of the criterion. What the recorded set contains is the evidence.
#[derive(Default)]
struct RecordingSource {
    seen: Mutex<Option<RowIdSet>>,
}

impl RecordingSource {
    /// The set the query path handed over, or `None` if it was never called.
    fn seen(&self) -> Option<RowIdSet> {
        self.seen
            .lock()
            .expect("no test holds this across a panic")
            .clone()
    }
}

impl CandidateSource for RecordingSource {
    async fn candidates(
        &self,
        within: &RowIdSet,
        limit: usize,
    ) -> euledb_storage::Result<Vec<RowId>> {
        *self.seen.lock().expect("no test holds this across a panic") = Some(within.clone());
        // Answer from what it was given, the way a real searcher restricted to a candidate set would.
        Ok(within.iter().take(limit).collect())
    }
}

#[tokio::test]
async fn candidate_generation_sees_only_the_rows_the_filter_kept() {
    let (_root, store) = populated().await;
    let source = RecordingSource::default();

    let found = store
        .filtered_search(
            "documents",
            &[
                Predicate::new("id >= 40 AND id < 70"),
                Predicate::new("language = 'de'"),
            ],
            &source,
            5,
        )
        .await
        .expect("a filtered search over existing columns resolves");

    let handed_over = source
        .seen()
        .expect("candidate generation must be called, not bypassed");

    // Hand-read off the corpus: thirty rows in the range, every third of them German, is ten. The
    // cardinality is the claim — a search that generated candidates first and filtered afterwards
    // would have handed over the whole table.
    assert_eq!(
        handed_over.len(),
        10,
        "candidate generation must see the ten rows the filter kept, not the thousand it did not",
    );

    assert_eq!(found.len(), 5, "the limit must reach the source");
    for row in &found {
        assert!(
            handed_over.contains(*row),
            "every candidate must come from the narrowed set: {row:?}",
        );
    }
}

/// A filter matching nothing still reaches the source, with an empty set.
///
/// Short-circuiting would be correct — the answer is empty either way — and it is deliberately not done.
/// A special case in the query path is a branch to maintain and test for no gain, and the port's contract
/// already says an implementation draws only from what it is given.
#[tokio::test]
async fn a_filter_matching_nothing_hands_over_an_empty_set() {
    let (_root, store) = populated().await;
    let source = RecordingSource::default();

    let found = store
        .filtered_search(
            "documents",
            std::slice::from_ref(&Predicate::new("id = -1")),
            &source,
            5,
        )
        .await
        .expect("a filter that matches nothing is not a failure");

    let handed_over = source
        .seen()
        .expect("the source must still be called, not short-circuited");
    assert!(
        handed_over.is_empty(),
        "the set handed over must be empty, not absent and not the whole table",
    );
    assert!(
        found.is_empty(),
        "no rows survived the filter, so none can be found"
    );
}

#[tokio::test]
async fn a_search_with_no_filter_at_all_is_refused() {
    let (_root, store) = populated().await;
    let source = RecordingSource::default();

    let refusal = store
        .filtered_search("documents", &[], &source, 5)
        .await
        .expect_err("a filtered search with nothing to filter by must be refused");
    assert!(
        matches!(
            &refusal,
            euledb_storage::Error::Storage(euledb_storage::StorageError::NothingToCombine {
                table,
                ..
            }) if table == "documents"
        ),
        "the refusal must name the table it was about: {refusal:?}",
    );
    assert!(
        source.seen().is_none(),
        "a refused search must not reach the source at all",
    );
}

/// A source that fails, fails the search — the filter's work is not quietly returned instead.
#[tokio::test]
async fn a_failing_source_fails_the_search() {
    struct Refusing;

    impl CandidateSource for Refusing {
        async fn candidates(
            &self,
            _within: &RowIdSet,
            _limit: usize,
        ) -> euledb_storage::Result<Vec<RowId>> {
            Err(euledb_storage::Error::from(
                euledb_storage::SchemaMismatch::MissingColumn {
                    column: "embedding".to_owned(),
                },
            ))
        }
    }

    let (_root, store) = populated().await;
    let refusal = store
        .filtered_search(
            "documents",
            std::slice::from_ref(&Predicate::new("id < 10")),
            &Refusing,
            5,
        )
        .await
        .expect_err("a source that fails must fail the search");
    assert!(
        matches!(
            &refusal,
            euledb_storage::Error::Schema(euledb_storage::SchemaMismatch::MissingColumn { column })
                if column == "embedding"
        ),
        "the source's own failure must pass through unchanged: {refusal:?}",
    );
}
