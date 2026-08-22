//! The published surface, used the way a caller would: open, declare, insert, read, change, remove.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb::{Assignment, Compression, Config, Database, Keyring, Predicate, TableSchema};

/// The shape of a document table.
fn documents() -> TableSchema {
    TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
    ]))
}

/// Two real-looking rows.
fn rows() -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(vec![4218_i64, 4219]));
    let title: ArrayRef = Arc::new(StringArray::from(vec![
        "Grundsatzurteil zur Vorratsdatenspeicherung",
        "Rapport sur la souveraineté numérique",
    ]));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("title", title, false)])
        .expect("the batch matches the declared schema")
}

#[tokio::test]
async fn rows_written_through_the_public_surface_come_back() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let db = Database::open_for_writing(root.path()).expect("the write role is free");

    db.create_table("documents", &documents())
        .await
        .expect("the table is declared");
    db.insert("documents", &rows())
        .await
        .expect("the rows land");

    let read: usize = db
        .scan("documents")
        .await
        .expect("the table reads back")
        .iter()
        .map(RecordBatch::num_rows)
        .sum();
    assert_eq!(read, 2, "what was inserted must come back out");
}

#[tokio::test]
async fn changing_and_removing_rows_through_the_public_surface() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let db = Database::open_for_writing(root.path()).expect("the write role is free");
    db.create_table("documents", &documents())
        .await
        .expect("the table is declared");
    db.insert("documents", &rows())
        .await
        .expect("the rows land");

    let updated = db
        .update(
            "documents",
            &Predicate::new("id = 4218"),
            &[Assignment::new("title", "'Neuer Titel'")],
        )
        .await
        .expect("the update applies");
    assert_eq!(updated.rows, 1, "exactly the matching row must be updated");

    let titles: Vec<String> = db
        .scan("documents")
        .await
        .expect("the table reads back")
        .iter()
        .flat_map(|batch| {
            let column = batch.column_by_name("title").expect("the title column");
            let strings = column
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("titles are strings");
            (0..batch.num_rows())
                .map(|row| strings.value(row).to_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        titles.contains(&"Neuer Titel".to_owned()),
        "the new value must be readable, not merely reported: {titles:?}",
    );

    let deleted = db
        .delete("documents", &Predicate::new("id = 4219"))
        .await
        .expect("the delete applies");
    assert_eq!(deleted.rows, 1, "exactly the matching row must be removed");

    db.drop_table("documents")
        .await
        .expect("a table that exists can be dropped");
    assert!(
        db.scan("documents").await.is_err(),
        "a dropped table must no longer be readable",
    );
}

/// The configuration knob has to have a measurable effect, or it is decoration.
///
/// Same rows, same schema, two configurations — the only difference is the compression the database
/// applies to a table it creates. The compressed table must be materially smaller on disk.
#[tokio::test]
async fn the_configured_compression_reaches_the_disk() {
    /// Enough rows that the margin is comfortable and the suite still runs in well under a second.
    const ROWS: i64 = 2_000;

    fn repetitive() -> RecordBatch {
        let id: ArrayRef = Arc::new(Int64Array::from((0..ROWS).collect::<Vec<i64>>()));
        let title: ArrayRef = Arc::new(StringArray::from(
            (0..ROWS)
                .map(|_| "Grundsatzurteil zur Vorratsdatenspeicherung")
                .collect::<Vec<&str>>(),
        ));
        RecordBatch::try_from_iter_with_nullable([("id", id, false), ("title", title, false)])
            .expect("the batch matches the declared schema")
    }

    async fn bytes_on_disk(config: Config) -> u64 {
        let root = tempfile::tempdir().expect("a temporary directory is available");
        {
            let db = Database::open_for_writing_with(root.path(), config)
                .expect("the write role is free");
            db.create_table("documents", &documents())
                .await
                .expect("the table is declared");
            db.insert("documents", &repetitive())
                .await
                .expect("the rows land");
        }
        let mut total = 0;
        let mut stack = vec![root.path().to_path_buf()];
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

    let compressed =
        bytes_on_disk(Config::default().with_compression(Compression::default())).await;
    let plain = bytes_on_disk(Config::default().with_compression(Compression::None)).await;

    assert!(
        compressed * 2 < plain,
        "the configured compression must reach the disk: {compressed} compressed vs {plain} plain",
    );
}

/// The facade's encryption is wired to the layer that does it, not merely named.
///
/// Whether the bytes on disk are really sealed is proven where the sealing happens. What this test
/// pins is the wiring: a handle opened with one keyring must not be readable through another. A
/// no-op `encrypted` would let the stranger read the rows.
#[tokio::test]
async fn an_encrypted_database_is_not_readable_with_other_keys() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let keyring = Keyring::create("korrektes-pferd-batterie-heftklammer").expect("keyring");
    {
        let db = Database::open_for_writing(root.path())
            .expect("the write role is free")
            .encrypted(&keyring);
        db.create_table("documents", &documents())
            .await
            .expect("the table is declared");
        db.insert("documents", &rows())
            .await
            .expect("the rows land");
    }

    let mine: usize = Database::open(root.path())
        .encrypted(&keyring)
        .scan("documents")
        .await
        .expect("the keyring that wrote it opens it")
        .iter()
        .map(RecordBatch::num_rows)
        .sum();
    assert_eq!(mine, 2, "the writing keyring must read its own rows back");

    // Same passphrase, different keyring: the data keys are random, so this is a reader holding keys
    // that do not open this database.
    let stranger = Keyring::create("korrektes-pferd-batterie-heftklammer").expect("keyring");
    assert!(
        Database::open(root.path())
            .encrypted(&stranger)
            .scan("documents")
            .await
            .is_err(),
        "keys that did not write this database must not read it",
    );
}

/// Auditing is on by default and switchable off, and off means no file at all.
#[tokio::test]
async fn the_audit_log_follows_the_configuration() {
    async fn log_exists(config: Config) -> bool {
        let root = tempfile::tempdir().expect("a temporary directory is available");
        {
            let db = Database::open_for_writing_with(root.path(), config)
                .expect("the write role is free");
            db.create_table("documents", &documents())
                .await
                .expect("the table is declared");
            db.insert("documents", &rows())
                .await
                .expect("the rows land");
        }
        root.path().join(".euledb-audit.log").exists()
    }

    assert!(
        log_exists(Config::default()).await,
        "auditing is on by default, so the default configuration must leave a log",
    );
    assert!(
        !log_exists(Config::default().with_auditing(false)).await,
        "off must mean no file — a database on read-only media has to stay usable",
    );
}

/// The whole getting-started path, in one test: declare, insert, index, and run all four query kinds.
///
/// This is the guide's content executed rather than described. If the guide and the API disagree, this
/// test says so — which is the point of the criterion asking for compiled examples rather than prose.
/// The walkthrough the getting-started guide shows, executed: declare, insert, index both ways.
///
/// Shared with the provenance test below rather than written twice — but kept whole and in order, because
/// this sequence is what a newcomer copies.
async fn seeded_library() -> (tempfile::TempDir, Database) {
    let model = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the crate sits two levels below the repository root")
        .join("model");
    let embedder = std::sync::Arc::new(
        euledb_embed::Embedder::load(&model)
            .expect("the model is fetched — run `just model` if this fails"),
    );

    let root = tempfile::tempdir().expect("a temporary directory is available");
    let db = Database::open_for_writing(root.path())
        .expect("the write role is free")
        .embedding(embedder);

    // A table whose `body` embeds itself on every insert.
    let schema = TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("body", DataType::Utf8, false),
    ]))
    .auto_embedding("body");
    db.create_table("documents", &schema)
        .await
        .expect("the table is declared");

    let id: ArrayRef = Arc::new(Int64Array::from(vec![1_i64, 2, 3]));
    let title: ArrayRef = Arc::new(StringArray::from(vec!["Gezeiten", "Datenschutz", "Wetter"]));
    let body: ArrayRef = Arc::new(StringArray::from(vec![
        "Als Flut wird das Steigen des Wasserstandes infolge der Gezeiten bezeichnet.",
        "Die Vorratsdatenspeicherung verpflichtet Anbieter zur Speicherung von Verbindungsdaten.",
        "Der Wasserstand fällt bei Ebbe und steigt bei Flut, zweimal an jedem Tag.",
    ]));
    let batch = RecordBatch::try_from_iter_with_nullable([
        ("id", id, false),
        ("title", title, false),
        ("body", body, false),
    ])
    .expect("the batch matches the declared schema");
    db.insert("documents", &batch).await.expect("the rows land");

    db.index_text("documents", "body", euledb::StemmingLanguage::German)
        .await
        .expect("the text is indexed");
    db.index_vectors("documents", "body", euledb::VectorIndexKind::Graph)
        .await
        .expect("the vectors are indexed");
    (root, db)
}

#[tokio::test]
async fn a_newcomer_can_run_every_query_kind() {
    let (_root, db) = seeded_library().await;

    // 1 — an exact filter.
    let exact = db
        .scan("documents")
        .await
        .expect("the table reads")
        .iter()
        .map(RecordBatch::num_rows)
        .sum::<usize>();
    assert_eq!(exact, 3, "three rows went in");

    // 2 — full text. `Wasserstand` reaches both sentences through the German stemmer.
    let lexical = db
        .text_search("documents", "body", "Wasserstand", 5)
        .await
        .expect("the text index answers");
    assert_eq!(
        lexical.len(),
        2,
        "two sentences share the stem: {lexical:?}"
    );
    let bounded = db
        .text_search("documents", "body", "Wasserstand", 1)
        .await
        .expect("the text index answers");
    assert_eq!(
        bounded.len(),
        1,
        "and the limit bounds the result, or paging through matches is impossible: {bounded:?}",
    );

    // 3 — semantic. A question none of the sentences words the same way.
    let semantic = db
        .semantic_search("documents", "body", "Wie hängen Ebbe und Flut zusammen?", 2)
        .await
        .expect("the vector index answers");
    assert_eq!(semantic.len(), 2, "two neighbours were asked for");

    // 4 — hybrid, with the rank each side gave every hit.
    let fused = db
        .hybrid_search("documents", "body", "Wasserstand bei Flut", 3)
        .await
        .expect("both paths answer and fuse");
    assert!(!fused.hits.is_empty(), "both sides found something to fuse");
    assert_eq!(
        fused.effective_k,
        euledb_storage::SMALL_CORPUS_K,
        "three rows is a small corpus, so the smaller k is used and reported",
    );
    for hit in &fused.hits {
        assert!(
            hit.vector_rank.is_some() || hit.lexical_rank.is_some(),
            "every hit says which side found it: {hit:?}",
        );
    }
}

#[tokio::test]
async fn a_semantic_query_without_an_embedder_says_what_is_missing() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let db = Database::open(root.path());

    let refusal = db
        .semantic_search("documents", "body", "irgendetwas", 3)
        .await
        .expect_err("a database that cannot embed cannot answer a semantic query");
    assert!(
        refusal.to_string().contains("embedding"),
        "the message must name the call that fixes it: {refusal}",
    );
}

/// The facade forwards the language, and only a query the two stemmers disagree about proves it.
///
/// German strips `-keit`, English does not, so `verhältnismäßig` reaches `Verhältnismäßigkeit` under one
/// and nothing under the other. Without this the facade could pass any language through and every other
/// full-text assertion here would still hold.
#[tokio::test]
async fn the_language_asked_for_reaches_the_text_index() {
    async fn indexed_in(language: euledb::StemmingLanguage) -> (tempfile::TempDir, Database) {
        let root = tempfile::tempdir().expect("a temporary directory is available");
        let db = Database::open_for_writing(root.path()).expect("the write role is free");
        let schema = TableSchema::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("body", DataType::Utf8, false),
        ]));
        db.create_table("documents", &schema)
            .await
            .expect("the table is declared");

        let id: ArrayRef = Arc::new(Int64Array::from(vec![1_i64]));
        let body: ArrayRef = Arc::new(StringArray::from(vec![
            "Das Gericht prüfte die Verhältnismäßigkeit der gespeicherten Daten sehr genau.",
        ]));
        let batch =
            RecordBatch::try_from_iter_with_nullable([("id", id, false), ("body", body, false)])
                .expect("the batch matches the declared schema");
        db.insert("documents", &batch).await.expect("the row lands");
        db.index_text("documents", "body", language)
            .await
            .expect("the text is indexed");
        (root, db)
    }

    let (_german_root, german) = indexed_in(euledb::StemmingLanguage::German).await;
    let (_english_root, english) = indexed_in(euledb::StemmingLanguage::English).await;

    let under_german = german
        .text_search("documents", "body", "verhältnismäßig", 4)
        .await
        .expect("the index answers");
    let under_english = english
        .text_search("documents", "body", "verhältnismäßig", 4)
        .await
        .expect("the index answers");

    assert_eq!(
        under_german.len(),
        1,
        "German strips `-keit`, so the query reaches the sentence: {under_german:?}",
    );
    assert!(
        under_english.is_empty(),
        "English does not, so asking for English must find nothing — otherwise the language the caller \
         gave was dropped on the way: {under_english:?}",
    );
}

/// The fused ranking must come from the caller's query, not from whatever vector was at hand.
///
/// Nothing else here can tell the difference: with a handful of rows the vector side returns *a* ranking
/// for any vector, so `effective_k`, the hit count and the per-side ranks all stay plausible. The closest
/// neighbour is the one claim that is exact — rank one under fusion is rank one under a plain semantic
/// search of the same query, whatever the breadth in between.
#[tokio::test]
async fn hybrid_search_ranks_the_caller_s_query_and_not_another_vector() {
    let (_root, db) = seeded_library().await;

    let query = "Wie hängen Ebbe und Flut zusammen?";
    let nearest = db
        .semantic_search("documents", "body", query, 1)
        .await
        .expect("the vector index answers");
    let fused = db
        .hybrid_search("documents", "body", query, 3)
        .await
        .expect("both paths answer and fuse");

    let closest_under_fusion = fused
        .hits
        .iter()
        .find(|hit| hit.vector_rank == Some(1))
        .map(|hit| hit.row);

    assert_eq!(
        closest_under_fusion,
        nearest.first().copied(),
        "the row the vector side put first must be the query's own nearest neighbour: {fused:?}",
    );
}
