//! Rotating a data key must not cost a rewrite, and must not cost the old rows.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{Compression, Keyring, LanceStore, TableDefinition, TableSchema, TableStore};

const PASSPHRASE: &str = "korrektes-pferd-batterie-heftklammer";

fn documents() -> TableSchema {
    TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("body", DataType::Utf8, false),
    ]))
}

fn uncompressed() -> TableDefinition {
    TableDefinition::new(documents()).with_compression(Compression::none())
}

/// A batch whose ids start where the caller says, so two batches are told apart by content.
fn batch(first_id: i64, count: i64) -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(
        (first_id..first_id + count).collect::<Vec<i64>>(),
    ));
    let body: ArrayRef = Arc::new(StringArray::from(
        (first_id..first_id + count)
            .map(|row| format!("Grundsatzurteil zur Vorratsdatenspeicherung Rn. {row}"))
            .collect::<Vec<String>>(),
    ));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("body", body, false)])
        .expect("the batch matches the declared schema")
}

/// Every data file, by path, with its bytes — so "was it rewritten" is answered by comparison rather
/// than by trusting a timestamp.
fn data_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.components().any(|part| part.as_os_str() == "data") {
                // By component, not by a string containing "/data/": Windows separates with a backslash,
                // so the string form found nothing there and the map came back empty. The assertion that
                // the map is non-empty is what caught it.

                let name = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                out.insert(name, std::fs::read(&path).unwrap_or_default());
            }
        }
    }
    out
}

fn all_ids(batches: &[RecordBatch]) -> Vec<i64> {
    let mut ids: Vec<i64> = batches
        .iter()
        .flat_map(|batch| {
            let column = batch.column(0);
            let values = column
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("the first column is the Int64 id");
            (0..values.len())
                .map(|row| values.value(row))
                .collect::<Vec<i64>>()
        })
        .collect();
    ids.sort_unstable();
    ids
}

#[tokio::test]
async fn rotating_the_data_key_leaves_earlier_rows_readable() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let mut keyring = Keyring::create(PASSPHRASE).expect("create");

    {
        let store = LanceStore::new(root.path()).encrypted(&keyring);
        store
            .create_table("documents", &uncompressed())
            .await
            .expect("create");
        store
            .append("documents", &batch(0, 500))
            .await
            .expect("first append");
    }

    let retired = keyring.current_data_key_id();
    let fresh = keyring.rotate_data_key().expect("rotating must succeed");
    assert_ne!(retired, fresh, "rotation did not produce a new key");

    // The rows written before the rotation are still sealed under the retired key, and the rows written
    // after are sealed under the new one. Both have to come back.
    let store = LanceStore::new(root.path()).encrypted(&keyring);
    store
        .append("documents", &batch(500, 500))
        .await
        .expect("second append");
    let read_back = store.scan("documents").await.expect("scan after rotation");

    assert_eq!(
        all_ids(&read_back),
        (0..1_000).collect::<Vec<i64>>(),
        "rows are missing after a rotation, so the retired key is not being used to read them",
    );
}

#[tokio::test]
async fn rotating_the_data_key_rewrites_no_payload() {
    // The criterion's actual constraint. Rotation that rewrote the data would be re-encryption, which
    // costs the whole database and is exactly what the envelope exists to avoid.
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let mut keyring = Keyring::create(PASSPHRASE).expect("create");

    let store = LanceStore::new(root.path()).encrypted(&keyring);
    store
        .create_table("documents", &uncompressed())
        .await
        .expect("create");
    store
        .append("documents", &batch(0, 500))
        .await
        .expect("append");

    let before = data_files(root.path());
    assert!(
        !before.is_empty(),
        "no data file was written, so there is nothing to compare"
    );

    keyring.rotate_data_key().expect("rotate");
    let after_rotation = data_files(root.path());
    assert_eq!(
        before, after_rotation,
        "rotating touched the payload, when the whole point is that it does not",
    );

    // And writing after the rotation adds a file rather than rewriting the old one.
    let rotated = LanceStore::new(root.path()).encrypted(&keyring);
    rotated
        .append("documents", &batch(500, 500))
        .await
        .expect("append after rotation");
    let after_write = data_files(root.path());

    for (name, bytes) in &before {
        assert_eq!(
            after_write.get(name),
            Some(bytes),
            "{name} was rewritten by a write that followed a rotation",
        );
    }
    assert!(
        after_write.len() > before.len(),
        "the write after the rotation produced no new data file, so nothing was written under the \
         new key",
    );
}

#[tokio::test]
async fn changing_the_passphrase_leaves_the_data_readable_and_untouched() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let mut keyring = Keyring::create(PASSPHRASE).expect("create");
    let written = batch(0, 500);

    {
        let store = LanceStore::new(root.path()).encrypted(&keyring);
        store
            .create_table("documents", &uncompressed())
            .await
            .expect("create");
        store.append("documents", &written).await.expect("append");
    }
    let before = data_files(root.path());

    let replacement = "neue-passphrase-mit-genug-entropie";
    keyring
        .change_passphrase(replacement)
        .expect("changing the passphrase must succeed");
    let keyfile = keyring.to_keyfile();

    assert_eq!(
        data_files(root.path()),
        before,
        "changing the passphrase touched the payload, when it only re-wraps the keys",
    );

    let reopened = Keyring::open(&keyfile, replacement).expect("the new passphrase must open it");
    let read_back = LanceStore::new(root.path())
        .encrypted(&reopened)
        .scan("documents")
        .await
        .expect("the data must still be readable after the passphrase changed");

    assert_eq!(
        read_back,
        vec![written],
        "the rows changed with the passphrase"
    );
}
