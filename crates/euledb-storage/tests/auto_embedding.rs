//! A column declared as auto-embedding stays consistent without the caller doing anything.

#![recursion_limit = "256"]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{Assignment, LanceStore, Predicate, TableDefinition, TableSchema, TableStore};

/// A table whose `body` embeds itself, and whose `title` does not.
fn documents() -> TableDefinition {
    TableDefinition::new(
        TableSchema::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("title", DataType::Utf8, false),
            Field::new("body", DataType::Utf8, false),
        ]))
        .auto_embedding("body"),
    )
}

fn rows() -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(vec![1_i64, 2]));
    let title: ArrayRef = Arc::new(StringArray::from(vec!["Gezeiten", "Datenschutz"]));
    let body: ArrayRef = Arc::new(StringArray::from(vec![
        "Als Flut wird das Steigen des Wasserstandes infolge der Gezeiten bezeichnet.",
        "Die Vorratsdatenspeicherung verpflichtet Anbieter, Verbindungsdaten zu speichern.",
    ]));
    RecordBatch::try_from_iter_with_nullable([
        ("id", id, false),
        ("title", title, false),
        ("body", body, false),
    ])
    .expect("the batch matches the declared schema")
}

/// A store that embeds, using the real model.
async fn embedding_store(root: &std::path::Path) -> LanceStore {
    let model = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the crate sits two levels below the repository root")
        .join("model");
    let embedder = euledb_embed::Embedder::load(&model)
        .expect("the model is fetched — run `just model` if this fails");
    LanceStore::open_for_writing(root)
        .expect("the write role is free")
        .embedding(Arc::new(embedder))
}

#[tokio::test]
async fn inserting_a_row_embeds_its_text_without_being_asked() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = embedding_store(root.path()).await;
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    let vectors = store
        .vectors_of("documents", "body")
        .await
        .expect("the vectors of an auto-embedding column are readable");
    assert_eq!(
        vectors.len(),
        2,
        "two rows, each short enough to be one chunk, is two vectors",
    );
    for vector in &vectors {
        assert_eq!(
            vector.embedding.len(),
            384,
            "every vector has the model's width",
        );
    }

    // The vectors have to be attributable to their rows, or a hit cannot be resolved back to data.
    let mut owners: Vec<u64> = vectors.iter().map(|v| v.row.get()).collect();
    owners.sort_unstable();
    owners.dedup();
    assert_eq!(owners.len(), 2, "each vector names the row it came from");
}

/// The hard half of AC-31: a row whose text changes and whose vector does not is a database that
/// answers yesterday's question with today's confidence.
#[tokio::test]
async fn updating_the_text_re_embeds_that_row() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = embedding_store(root.path()).await;
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    let before = store
        .vectors_of("documents", "body")
        .await
        .expect("vectors are readable");

    store
        .update(
            "documents",
            &Predicate::new("id = 1"),
            &[Assignment::new(
                "body",
                "'Die Vorratsdatenspeicherung verpflichtet Anbieter zur Speicherung.'",
            )],
        )
        .await
        .expect("the update applies");

    let after = store
        .vectors_of("documents", "body")
        .await
        .expect("vectors are readable");
    assert_eq!(
        after.len(),
        2,
        "the row count did not change, so nor did the vector count"
    );

    // Compared as a set of embeddings, not paired by row id. **An update gives the row a new
    // identity** — the format rewrites it into a new fragment — so pairing by row id compares
    // unrelated rows and reports every vector as changed.
    let survivors = before
        .iter()
        .filter(|old| after.iter().any(|new| new.embedding == old.embedding))
        .count();
    assert_eq!(
        survivors, 1,
        "one row's text was untouched, so exactly one vector must survive unchanged — {survivors} did",
    );
    let fresh = after
        .iter()
        .filter(|new| !before.iter().any(|old| old.embedding == new.embedding))
        .count();
    assert_eq!(
        fresh, 1,
        "and the row whose text changed must have exactly one new vector — {fresh} appeared",
    );
}

/// The behaviour that broke the two tests above, named so the next reader does not rediscover it.
///
/// An update does not edit a row in place: the format rewrites it, and the rewritten row carries a **new
/// row id**. Anything keyed on row identity — a vector, a bitmap, an index entry — is therefore stale
/// after an update, which is exactly why the reconciliation is a reconciliation rather than a patch.
#[tokio::test]
async fn an_updated_row_comes_back_with_a_new_identity() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = embedding_store(root.path()).await;
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    let before = store
        .row_ids("documents", &Predicate::new("id = 1"))
        .await
        .expect("the row resolves");
    store
        .update(
            "documents",
            &Predicate::new("id = 1"),
            &[Assignment::new("title", "'Ein anderer Titel'")],
        )
        .await
        .expect("the update applies");
    let after = store
        .row_ids("documents", &Predicate::new("id = 1"))
        .await
        .expect("the row still resolves");

    assert_eq!(before.len(), 1, "one row carries id 1");
    assert_eq!(after.len(), 1, "and still does after the update");
    assert_ne!(
        before[0], after[0],
        "an updated row is rewritten, so its identity changes — this is the format's behaviour, not \
         a defect, and every index over row ids has to survive it",
    );
}

#[tokio::test]
async fn updating_a_column_that_does_not_embed_leaves_the_vectors_alone() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = embedding_store(root.path()).await;
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    let before = store
        .vectors_of("documents", "body")
        .await
        .expect("vectors are readable");
    store
        .update(
            "documents",
            &Predicate::new("id = 1"),
            &[Assignment::new("title", "'Ein anderer Titel'")],
        )
        .await
        .expect("the update applies");
    let after = store
        .vectors_of("documents", "body")
        .await
        .expect("vectors are readable");

    // Compared by content, not by row id, and not by whether the work was redone. Two things are not
    // observable here: an update rewrites the row under a new identity, and re-embedding unchanged text
    // produces byte-identical output. So this asserts the only thing that matters to a caller — the
    // vectors still describe the same texts.
    let mut before_content: Vec<&Vec<f32>> = before.iter().map(|v| &v.embedding).collect();
    let mut after_content: Vec<&Vec<f32>> = after.iter().map(|v| &v.embedding).collect();
    before_content.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    after_content.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    assert_eq!(
        before_content, after_content,
        "the embedding column did not change, so the vectors must still describe the same texts",
    );
}

#[tokio::test]
async fn a_table_without_an_embedding_column_stores_no_vectors() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = embedding_store(root.path()).await;
    let plain = TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("body", DataType::Utf8, false),
    ])));
    store
        .create_table("documents", &plain)
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    assert!(
        store.vectors_of("documents", "body").await.is_err(),
        "a column nobody declared as embedding has no vectors to read",
    );
}

#[tokio::test]
async fn deleting_a_row_takes_its_vector_with_it() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = embedding_store(root.path()).await;
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    store
        .delete("documents", &Predicate::new("id = 1"))
        .await
        .expect("the delete applies");

    let vectors = store
        .vectors_of("documents", "body")
        .await
        .expect("vectors are readable");
    assert_eq!(
        vectors.len(),
        1,
        "one row left, so one vector — an orphaned vector is a hit that resolves to nothing",
    );
}

/// Inserting into a table that embeds itself, with no embedder, is refused rather than skipped.
///
/// A row stored with no vector is a row no semantic query can ever find. Silence there would be a
/// database quietly forgetting half of what it was given.
#[tokio::test]
async fn a_handle_without_an_embedder_refuses_to_insert() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("documents", &documents())
        .await
        .expect("declaring the table needs no embedder");

    let refusal = store
        .append("documents", &rows())
        .await
        .expect_err("inserting into an auto-embedding table without an embedder must be refused");
    assert!(
        refusal.to_string().contains("embedding"),
        "the refusal must say what is missing and how to supply it: {refusal}",
    );
}
