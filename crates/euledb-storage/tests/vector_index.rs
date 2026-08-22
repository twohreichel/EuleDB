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
use euledb_storage::{LanceStore, TableDefinition, TableSchema, TableStore, VectorIndexKind};

/// Enough documents for recall to mean something, and enough for product quantisation to train.
///
/// The lower bound is not arbitrary: a codebook of `2^num_bits` centroids cannot be trained on fewer
/// vectors than centroids, and the quantised index here uses four bits — sixteen. Twelve documents
/// failed with exactly that message, which is a constraint of the method rather than of this test.
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
        .create_vector_index("documents", "body", VectorIndexKind::Graph)
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

    // **The nearest of all must be found exactly.** That is the claim an index either keeps or is
    // broken on, and it held on every platform.
    assert_eq!(
        found[0].row.get(),
        expected[0],
        "the nearest vector must be the nearest vector",
    );

    // The tail is a different matter, and the first version of this assertion was wrong about it. HNSW
    // is **approximate by construction**: over twenty vectors the ranking below the top few is not
    // stable, and CI disagreed with this machine on positions four and five. A threshold tuned until it
    // passed here would be a flake generator, so this asserts a majority and says why.
    //
    // The recall *number* is not this test's job. Recall@10 over the reference corpus with a thousand
    // queries is measured where the KPIs are, against a corpus large enough for the figure to mean
    // something.
    let overlap = found
        .iter()
        .filter(|hit| expected.contains(&hit.row.get()))
        .count();
    assert!(
        overlap >= 3,
        "an approximate index may miss the tail, but not the majority: agreed on {overlap} of five, \
         index {:?} against exhaustive {expected:?}",
        found.iter().map(|hit| hit.row.get()).collect::<Vec<u64>>(),
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
            .create_vector_index("documents", "body", VectorIndexKind::Graph)
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
        .create_vector_index("documents", "body", VectorIndexKind::Graph)
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

/// Both index kinds answer, and the query does not say which one it is asking.
///
/// The criterion's substance is that last clause: a caller picks the trade-off once, when the index is
/// built, and every query afterwards is the same call. If the query API had to know, the choice would
/// leak into every consumer.
#[tokio::test]
async fn either_index_kind_answers_the_same_query_call() {
    async fn nearest_with(kind: VectorIndexKind) -> Vec<u64> {
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
            .create_vector_index("documents", "body", kind)
            .await
            .expect("either kind can be built");

        let query = model
            .embed_query("Datenschutz und Vorratsdatenspeicherung")
            .expect("the query embeds");
        // The same call for both kinds — no parameter here names the index.
        store
            .nearest("documents", "body", query.as_slice(), 3)
            .await
            .expect("either kind answers")
            .iter()
            .map(|hit| hit.row.get())
            .collect()
    }

    let graph = nearest_with(VectorIndexKind::Graph).await;
    let quantised = nearest_with(VectorIndexKind::Quantised).await;

    assert_eq!(graph.len(), 3, "the graph index returns what was asked for");
    assert_eq!(quantised.len(), 3, "and so does the quantised one");

    // **They do not agree on the nearest vector, and asserting that they would was wrong.** Product
    // quantisation answers from a lossy code rather than from the vector, and with a four-bit codebook
    // over two dozen vectors the loss is large — measured, not supposed: the graph returned [4, 5, 14]
    // and the quantised index [5, 4, 2].
    //
    // So the claim here is the criterion's own: the *same call* serves both, and neither the caller nor
    // this test says which index answers. How closely the lossy one tracks the exact answer is a recall
    // figure, and a recall figure over two dozen vectors means nothing — that measurement belongs to the
    // benchmark over the reference corpus.
    let shared = graph.iter().filter(|row| quantised.contains(row)).count();
    assert!(
        shared >= 1,
        "the two kinds must be answering the same question, however approximately: graph {graph:?} \
         against quantised {quantised:?}",
    );
}

/// What the two index kinds cost on disk, measured — and the measurement contradicts the obvious claim.
///
/// The reason a quantised index exists is memory: each vector becomes a short code. **At this scale it is
/// the larger artefact, not the smaller one** — 27 847 bytes against 16 479 for the graph over two dozen
/// vectors. The codebook is a fixed cost (sixteen sub-vectors, sixteen centroids each, twenty-four
/// components apiece) and it dominates until the collection is far larger than the codebook.
///
/// So this test asserts only that both kinds leave an index behind. Asserting a direction would mean
/// asserting something untrue at the size a test can afford, and the crossover — where quantisation
/// starts to pay — is a memory measurement over the reference corpus rather than a unit test.
#[tokio::test]
async fn both_index_kinds_leave_an_index_on_disk() {
    async fn index_bytes(kind: VectorIndexKind) -> u64 {
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
        store
            .create_vector_index("documents", "body", kind)
            .await
            .expect("either kind can be built");

        // Only the index, not the vectors it was built from: the companion table is the same either way.
        let mut total = 0;
        let mut stack = vec![
            root.path()
                .join("documents.body.vectors.lance")
                .join("_indices"),
        ];
        while let Some(path) = stack.pop() {
            for entry in std::fs::read_dir(&path).into_iter().flatten().flatten() {
                let child = entry.path();
                if child.is_dir() {
                    stack.push(child);
                } else {
                    total += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                }
            }
        }
        total
    }

    let graph = index_bytes(VectorIndexKind::Graph).await;
    let quantised = index_bytes(VectorIndexKind::Quantised).await;

    assert!(
        graph > 0,
        "the graph index must leave an artefact behind, or nothing was built",
    );
    assert!(
        quantised > 0,
        "and so must the quantised one, or the second kind is a no-op with a different name",
    );

    // Size cannot make the choice observable: two builds of the *same* kind also differ in bytes, so an
    // inequality here is satisfied by noise. That was measured, not assumed — a mutation ignoring the
    // requested kind survived an `assert_ne!` on these numbers. The index's own recorded type is the
    // signal, and it has its own test below.
}

/// The selection is observable, which is what makes it a selection.
///
/// This is the test that catches a build ignoring the kind it was asked for. Neither the answers nor the
/// artefact sizes can: the answers agree often enough, and two builds of one kind differ in size anyway.
#[tokio::test]
async fn the_index_records_the_kind_it_was_asked_for() {
    for kind in [VectorIndexKind::Graph, VectorIndexKind::Quantised] {
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

        assert_eq!(
            store
                .vector_index_kind("documents", "body")
                .await
                .expect("the metadata is readable"),
            None,
            "before anything is built there is no index kind to report",
        );

        store
            .create_vector_index("documents", "body", kind)
            .await
            .expect("either kind can be built");

        assert_eq!(
            store
                .vector_index_kind("documents", "body")
                .await
                .expect("the metadata is readable"),
            Some(kind),
            "the index must record the kind it was asked for, not the one it felt like building",
        );
    }
}
