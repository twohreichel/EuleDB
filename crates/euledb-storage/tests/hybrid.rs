//! One query, both retrieval paths, one ranking that says where each hit came from.

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
    LanceStore, SMALL_CORPUS_K, StemmingLanguage, TableDefinition, TableSchema, TableStore,
    VectorIndexKind,
};

/// Above product quantisation's training minimum, and small enough that the suite stays usable.
const DOCUMENTS: usize = 24;

fn documents() -> TableDefinition {
    TableDefinition::new(
        TableSchema::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("body", DataType::Utf8, false),
        ]))
        .auto_embedding("body"),
    )
}

fn corpus() -> Vec<(i64, String)> {
    euledb_corpus::smoke()
        .into_iter()
        .take(DOCUMENTS)
        .enumerate()
        .map(|(index, document)| {
            (
                i64::try_from(index).expect("fits"),
                document.text.chars().take(600).collect(),
            )
        })
        .collect()
}

fn batch(rows: &[(i64, String)]) -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(
        rows.iter().map(|(id, _)| *id).collect::<Vec<i64>>(),
    ));
    let body: ArrayRef = Arc::new(StringArray::from(
        rows.iter().map(|(_, t)| t.as_str()).collect::<Vec<&str>>(),
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

async fn both_paths() -> (tempfile::TempDir, LanceStore, Arc<euledb_embed::Embedder>) {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let model = embedder();
    let store = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .embedding(model.clone());
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store
        .append("documents", &batch(&corpus()))
        .await
        .expect("rows land and embed");
    store
        .create_vector_index("documents", "body", VectorIndexKind::Graph)
        .await
        .expect("the vectors are indexed");
    store
        .create_text_index("documents", "body", StemmingLanguage::German)
        .await
        .expect("the text is indexed");
    (root, store, model)
}

#[tokio::test]
async fn a_hybrid_query_returns_one_ranking_from_both_paths() {
    let (_root, store, model) = both_paths().await;
    let vector = model
        .embed_query("Geschichte und Politik in Europa")
        .expect("the query embeds");

    let fused = store
        .hybrid_search(
            "documents",
            "body",
            "Geschichte Politik Europa",
            vector.as_slice(),
            5,
        )
        .await
        .expect("both paths answer and fuse");

    assert!(!fused.hits.is_empty(), "both sides found something to fuse");
    assert!(fused.hits.len() <= 5, "the limit bounds the fused ranking");

    // Descending by score: a ranking that is not ordered is not a ranking.
    for pair in fused.hits.windows(2) {
        assert!(
            pair[0].score >= pair[1].score,
            "the ranking must descend: {:?}",
            fused.hits.iter().map(|hit| hit.score).collect::<Vec<f32>>(),
        );
    }

    // Every hit came from somewhere, and says where.
    for hit in &fused.hits {
        assert!(
            hit.vector_rank.is_some() || hit.lexical_rank.is_some(),
            "a hit that neither side found cannot be in the ranking: {hit:?}",
        );
    }
}

/// The small-corpus `k` is used and reported, which is the whole of one criterion.
#[tokio::test]
async fn a_small_corpus_reports_the_k_it_actually_used() {
    let (_root, store, model) = both_paths().await;
    let vector = model.embed_query("Politik").expect("the query embeds");

    let fused = store
        .hybrid_search("documents", "body", "Politik", vector.as_slice(), 5)
        .await
        .expect("both paths answer and fuse");

    assert_eq!(
        fused.effective_k, SMALL_CORPUS_K,
        "two dozen rows is well below the threshold, so the smaller k must be used",
    );
}

/// A row both paths found must outrank one only a single path found, on real data.
///
/// The unit tests prove the arithmetic on hand-built lists. This proves the two real sources are actually
/// being combined rather than one of them being returned and the other ignored.
#[tokio::test]
async fn a_row_both_paths_found_outranks_one_only_one_path_found() {
    let (_root, store, model) = both_paths().await;
    let vector = model
        .embed_query("Geschichte und Politik in Europa")
        .expect("the query embeds");

    let fused = store
        .hybrid_search(
            "documents",
            "body",
            "Geschichte Politik Europa",
            vector.as_slice(),
            10,
        )
        .await
        .expect("both paths answer and fuse");

    let both: Vec<usize> = fused
        .hits
        .iter()
        .enumerate()
        .filter(|(_, hit)| hit.vector_rank.is_some() && hit.lexical_rank.is_some())
        .map(|(position, _)| position)
        .collect();
    let single: Vec<usize> = fused
        .hits
        .iter()
        .enumerate()
        .filter(|(_, hit)| hit.vector_rank.is_none() || hit.lexical_rank.is_none())
        .map(|(position, _)| position)
        .collect();

    assert!(
        !both.is_empty(),
        "at least one row must be found by both paths, or this query does not exercise fusion: {:?}",
        fused.hits,
    );
    if let (Some(worst_shared), Some(best_single)) = (both.last(), single.first()) {
        assert!(
            worst_shared < best_single || single.is_empty(),
            "every row both paths found must outrank every row only one found: shared at {both:?}, \
             single at {single:?}",
        );
    }
}
