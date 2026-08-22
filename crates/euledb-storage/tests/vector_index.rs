//! An indexed vector column answers a nearest-neighbour query, and the answers are the right ones.

#![recursion_limit = "256"]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{LanceStore, TableDefinition, TableSchema, TableStore};

/// Enough documents for recall to mean something, few enough that the suite stays quick.
const DOCUMENTS: usize = 12;

fn documents() -> TableDefinition {
    TableDefinition::new(
        TableSchema::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("body", DataType::Utf8, false),
        ]))
        .auto_embedding("body"),
    )
}

/// Real corpus documents, truncated so each is one chunk — recall is about which document is nearest,
/// and multi-chunk documents would test the chunking instead.
fn corpus() -> Vec<(i64, String)> {
    euledb_corpus::smoke()
        .into_iter()
        .take(DOCUMENTS)
        .enumerate()
        .map(|(index, document)| {
            let text: String = document.text.chars().take(600).collect();
            (i64::try_from(index).expect("fits"), text)
        })
        .collect()
}

fn batch(rows: &[(i64, String)]) -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(
        rows.iter().map(|(id, _)| *id).collect::<Vec<i64>>(),
    ));
    let body: ArrayRef = Arc::new(StringArray::from(
        rows.iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<&str>>(),
    ));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("body", body, false)])
        .expect("the batch matches the declared schema")
}

fn embedder() -> Arc<euledb_embed::Embedder> {
    let model = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the crate sits two levels below the repository root")
        .join("model");
    Arc::new(
        euledb_embed::Embedder::load(&model)
            .expect("the model is fetched — run `just model` if this fails"),
    )
}

/// AC-34's observable requirement: the index finds what an exhaustive search finds.
///
/// Recall against a brute-force baseline computed in the test, not against a stored expectation — the
/// baseline is every vector compared to the query, which is the definition rather than a fixture.
#[tokio::test]
async fn an_indexed_vector_column_finds_what_an_exhaustive_search_finds() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let model = embedder();
    let store = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .embedding(model.clone());
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    let rows = corpus();
    store
        .append("documents", &batch(&rows))
        .await
        .expect("rows land and embed");

    let vectors = store
        .vectors_of("documents", "body")
        .await
        .expect("vectors are readable");
    assert!(
        vectors.len() >= DOCUMENTS,
        "each document contributes at least one vector, so at least {DOCUMENTS} — got {}",
        vectors.len(),
    );

    store
        .create_vector_index("documents", "body")
        .await
        .expect("an embedding column can be indexed");

    // A query drawn from the corpus itself: its own document must be the nearest thing to it.
    let query = model
        .embed_query(&rows[3].1.chars().take(120).collect::<String>())
        .expect("the query embeds");

    let found = store
        .nearest("documents", "body", query.as_slice(), 5)
        .await
        .expect("the index answers");
    assert_eq!(found.len(), 5, "five neighbours were asked for");

    // The baseline: every vector, compared exhaustively. Cosine is a dot product because both sides
    // are L2-normalised.
    let mut exhaustive: Vec<(f32, u64)> = vectors
        .iter()
        .map(|vector| {
            let similarity: f32 = vector
                .embedding
                .iter()
                .zip(query.as_slice())
                .map(|(a, b)| a * b)
                .sum();
            (similarity, vector.row.get())
        })
        .collect();
    exhaustive.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let expected: Vec<u64> = exhaustive.iter().take(5).map(|(_, row)| *row).collect();

    let overlap = found
        .iter()
        .filter(|hit| expected.contains(&hit.row.get()))
        .count();
    assert!(
        overlap >= 4,
        "the index must agree with the exhaustive answer on at least four of five, but agreed on \
         {overlap}: index {:?} against exhaustive {expected:?}",
        found.iter().map(|hit| hit.row.get()).collect::<Vec<u64>>(),
    );

    // And the nearest of all must be found — an index that misses the best match is not an index.
    assert_eq!(
        found[0].row.get(),
        expected[0],
        "the nearest vector must be the nearest vector",
    );
}

#[tokio::test]
async fn indexing_a_column_that_does_not_embed_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .embedding(embedder());
    let plain = TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("body", DataType::Utf8, false),
    ])));
    store
        .create_table("documents", &plain)
        .await
        .expect("the table is declared");

    assert!(
        store
            .create_vector_index("documents", "body")
            .await
            .is_err(),
        "a column with no vectors has nothing to index",
    );
}

/// The answers alone cannot show that an index exists, so the plan is asked directly.
///
/// With a small collection an exhaustive comparison returns exactly what the index returns. A mutation
/// that never builds the index therefore passes every assertion about neighbours — measured, not
/// assumed. This is the test that catches it.
#[tokio::test]
async fn the_search_goes_through_the_index_once_one_exists() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let model = embedder();
    let store = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .embedding(model.clone());
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    let rows = corpus();
    store
        .append("documents", &batch(&rows))
        .await
        .expect("rows land and embed");

    let query = model
        .embed_query("Datenschutz und Vorratsdatenspeicherung")
        .expect("the query embeds");

    let before = store
        .nearest_uses_the_index("documents", "body", query.as_slice(), 5)
        .await
        .expect("the plan is readable");
    assert!(
        !before,
        "with no index built, the search cannot be going through one",
    );

    store
        .create_vector_index("documents", "body")
        .await
        .expect("an embedding column can be indexed");

    let after = store
        .nearest_uses_the_index("documents", "body", query.as_slice(), 5)
        .await
        .expect("the plan is readable");
    assert!(
        after,
        "once an index exists the search must use it — otherwise building it changed nothing",
    );
}

#[tokio::test]
async fn a_query_of_the_wrong_width_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .embedding(embedder());
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store
        .append("documents", &batch(&corpus()))
        .await
        .expect("rows land and embed");

    let refusal = store
        .nearest("documents", "body", &[0.0_f32; 8], 5)
        .await
        .expect_err("a query that is not the model's width cannot be compared to anything");
    // Its own variant, not a backend failure: nothing below was asked anything, and the message has to
    // carry both numbers or a caller cannot act on it.
    assert!(
        matches!(
            &refusal,
            euledb_storage::Error::Storage(euledb_storage::StorageError::WrongVectorWidth {
                given: 8,
                wanted: 384,
            })
        ),
        "the refusal must name both widths: {refusal:?}",
    );
}
