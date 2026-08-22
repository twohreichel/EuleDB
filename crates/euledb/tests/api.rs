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
