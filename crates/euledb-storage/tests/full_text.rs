//! BM25 full text over a declared column, with a ranking that does not move between runs.

#![recursion_limit = "256"]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{LanceStore, StemmingLanguage, TableDefinition, TableSchema, TableStore};

fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("body", DataType::Utf8, false),
    ])))
}

/// German sentences chosen so that stemming is the difference between a hit and a miss.
fn rows() -> RecordBatch {
    let texts = [
        "Die Vorratsdatenspeicherung verpflichtet Anbieter zur Speicherung von Verbindungsdaten.",
        "Das Gericht prüfte die Verhältnismäßigkeit der gespeicherten Daten sehr genau.",
        "Als Flut wird das Steigen des Wasserstandes infolge der Gezeiten bezeichnet.",
        "Der Wasserstand fällt bei Ebbe und steigt bei Flut, zweimal an jedem Tag.",
    ];
    let id: ArrayRef = Arc::new(Int64Array::from((0..4_i64).collect::<Vec<i64>>()));
    let body: ArrayRef = Arc::new(StringArray::from(texts.to_vec()));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("body", body, false)])
        .expect("the batch matches the declared schema")
}

async fn indexed_in(language: StemmingLanguage) -> (tempfile::TempDir, LanceStore) {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");
    store
        .create_text_index("documents", "body", language)
        .await
        .expect("a text column can be indexed");
    (root, store)
}

async fn indexed() -> (tempfile::TempDir, LanceStore) {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");
    store
        .create_text_index("documents", "body", StemmingLanguage::German)
        .await
        .expect("a text column can be indexed");
    (root, store)
}

#[tokio::test]
async fn a_full_text_query_ranks_the_matching_documents() {
    let (_root, store) = indexed().await;

    let hits = store
        .search_text("documents", "body", "Vorratsdatenspeicherung", 4)
        .await
        .expect("the index answers");

    assert!(
        !hits.is_empty(),
        "the word is in the corpus, so something must match"
    );
    // Hand-read: only the first sentence contains the term.
    assert_eq!(
        hits.len(),
        1,
        "exactly one sentence carries that word: {hits:?}",
    );
}

/// Stemming is the point of choosing a language, and this is the test that shows it reached the index.
///
/// The pair is **measured, not supposed**: one sentence writes `Wasserstandes`, another `Wasserstand`, and
/// German Snowball reduces both to the same stem — so either query finds both sentences. Without stemming
/// each query would find only the sentence spelling it that way.
///
/// My first attempt used `speichern` against `Speicherung` and failed: Snowball's German stemmer does not
/// strip `-ung`. It does strip `-es` and `-en`, which is why this pair works and that one did not. The
/// premise was wrong, not the index.
///
/// This pair does not, however, show that the *German* stemmer was used — English strips `-es` too, and a
/// mutation forcing English survived it. `the_language_reaches_the_index` below is the test for that.
#[tokio::test]
async fn stemming_relates_the_inflected_forms_it_actually_relates() {
    let (_root, store) = indexed().await;

    let genitive = store
        .search_text("documents", "body", "Wasserstandes", 4)
        .await
        .expect("the index answers");
    let nominative = store
        .search_text("documents", "body", "Wasserstand", 4)
        .await
        .expect("the index answers");

    assert_eq!(
        genitive.len(),
        2,
        "the genitive must reach both sentences, not only the one spelling it: {genitive:?}",
    );
    assert_eq!(
        nominative.len(),
        2,
        "and so must the nominative: {nominative:?}",
    );

    let mut from_genitive: Vec<u64> = genitive.iter().map(|row| row.get()).collect();
    let mut from_nominative: Vec<u64> = nominative.iter().map(|row| row.get()).collect();
    from_genitive.sort_unstable();
    from_nominative.sort_unstable();
    assert_eq!(
        from_genitive, from_nominative,
        "both forms share a stem, so they must reach the same rows",
    );
}

/// AC-36's second clause: identical runs rank identically.
#[tokio::test]
async fn the_ranking_does_not_move_between_identical_runs() {
    let (_root, store) = indexed().await;

    let first = store
        .search_text("documents", "body", "Wasserstand Flut", 4)
        .await
        .expect("the index answers");
    let second = store
        .search_text("documents", "body", "Wasserstand Flut", 4)
        .await
        .expect("the index answers again");

    assert!(
        first.len() >= 2,
        "two sentences mention both terms: {first:?}"
    );
    assert_eq!(
        first, second,
        "the same query must rank the same way, or a caller cannot paginate",
    );
}

#[tokio::test]
async fn searching_an_unindexed_column_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    assert!(
        store
            .search_text("documents", "body", "Flut", 4)
            .await
            .is_err(),
        "a column with no text index cannot answer a full-text query",
    );
}

/// The chosen language reaches the index, and this is the only assertion here that shows it.
///
/// The pair is measured: `verhältnismäßig` finds the sentence containing `Verhältnismäßigkeit` under
/// German — the stemmer strips `-keit` — and finds **nothing** under English, which does not. A mutation
/// forcing English survived every other test in this file, including the stemming one, because English
/// also strips the `-es` that pair relies on.
#[tokio::test]
async fn the_language_reaches_the_index() {
    let (_german_root, german) = indexed_in(StemmingLanguage::German).await;
    let (_english_root, english) = indexed_in(StemmingLanguage::English).await;

    let under_german = german
        .search_text("documents", "body", "verhältnismäßig", 4)
        .await
        .expect("the index answers");
    let under_english = english
        .search_text("documents", "body", "verhältnismäßig", 4)
        .await
        .expect("the index answers");

    assert_eq!(
        under_german.len(),
        1,
        "German strips `-keit`, so this reaches `Verhältnismäßigkeit`: {under_german:?}",
    );
    assert!(
        under_english.is_empty(),
        "English does not, so the same query must find nothing — otherwise the language was ignored: \
         {under_english:?}",
    );
}

/// The limit is honoured, which nothing else here checks.
#[tokio::test]
async fn the_limit_bounds_the_result() {
    let (_root, store) = indexed().await;

    let all = store
        .search_text("documents", "body", "Wasserstand", 4)
        .await
        .expect("the index answers");
    assert_eq!(all.len(), 2, "two sentences share the stem");

    let one = store
        .search_text("documents", "body", "Wasserstand", 1)
        .await
        .expect("the index answers");
    assert_eq!(
        one.len(),
        1,
        "asking for one must return one, not everything that matched: {one:?}",
    );
}
