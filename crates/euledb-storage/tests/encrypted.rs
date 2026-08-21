//! Encryption at rest, end to end through the on-disk format.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::path::Path;
use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{Compression, Keyring, LanceStore, TableDefinition, TableSchema, TableStore};

const PASSPHRASE: &str = "korrektes-pferd-batterie-heftklammer";

/// A string that must not be findable on disk once the table is encrypted.
const MARKER: &str = "Vorratsdatenspeicherung-Aktenzeichen-1BvR256";

fn documents() -> TableSchema {
    TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("body", DataType::Utf8, false),
    ]))
}

/// A table declared WITHOUT compression.
///
/// Load-bearing, not incidental: with zstd on, the marker string is unfindable on disk whether or not
/// anything was encrypted — and the first version of this test passed against a completely unencrypted
/// table for exactly that reason.
fn uncompressed() -> TableDefinition {
    TableDefinition::new(documents()).with_compression(Compression::none())
}

fn rows() -> RecordBatch {
    let count = 2_000;
    let id: ArrayRef = Arc::new(Int64Array::from((0..count).collect::<Vec<i64>>()));
    let body: ArrayRef = Arc::new(StringArray::from(
        (0..count)
            .map(|row| format!("{MARKER} Rn. {row}"))
            .collect::<Vec<String>>(),
    ));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("body", body, false)])
        .expect("the batch matches the declared schema")
}

/// Every byte under a directory, so a claim about what is on disk is checked rather than asserted.
fn all_bytes(root: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path).into_iter().flatten().flatten() {
            let child = entry.path();
            if child.is_dir() {
                stack.push(child);
            } else if let Ok(bytes) = std::fs::read(&child) {
                out.extend_from_slice(&bytes);
            }
        }
    }
    out
}

fn contains_marker(bytes: &[u8]) -> bool {
    bytes
        .windows(MARKER.len())
        .any(|window| window == MARKER.as_bytes())
}

#[tokio::test]
async fn encrypted_rows_survive_a_drop_and_reopen_unchanged() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let keyring = Keyring::create(PASSPHRASE).expect("creating a keyring must succeed");
    let keyfile = keyring.to_keyfile();
    let written = rows();

    {
        let store = LanceStore::new(root.path()).encrypted(&keyring);
        store
            .create_table("documents", &uncompressed())
            .await
            .expect("creating an encrypted table must succeed");
        store
            .append("documents", &written)
            .await
            .expect("appending to an encrypted table must succeed");
    }

    // Reopened from the keyfile, the way a caller would: the passphrase is what they have.
    let reopened =
        Keyring::open(&keyfile, PASSPHRASE).expect("the passphrase must open the keyring");
    let read_back = LanceStore::new(root.path())
        .encrypted(&reopened)
        .scan("documents")
        .await
        .expect("an encrypted table must be readable with the right key");

    assert_eq!(
        read_back,
        vec![written],
        "the rows that came back are not the rows that went in",
    );
}

#[tokio::test]
async fn the_marker_is_on_disk_without_encryption() {
    // The control. Without it, the test below passes against a table that was never encrypted — which
    // is exactly what happened the first time.
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::new(root.path());
    store
        .create_table("documents", &uncompressed())
        .await
        .expect("create");
    store.append("documents", &rows()).await.expect("append");

    assert!(
        contains_marker(&all_bytes(root.path())),
        "the marker is not on disk even unencrypted, so the encryption test proves nothing",
    );
}

#[tokio::test]
async fn the_marker_is_not_on_disk_with_encryption() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let keyring = Keyring::create(PASSPHRASE).expect("create");
    let store = LanceStore::new(root.path()).encrypted(&keyring);
    store
        .create_table("documents", &uncompressed())
        .await
        .expect("create");
    store.append("documents", &rows()).await.expect("append");

    let on_disk = all_bytes(root.path());
    assert!(
        !on_disk.is_empty(),
        "nothing was written at all, so the next assertion would prove nothing",
    );
    assert!(
        !contains_marker(&on_disk),
        "the row text is readable on disk, so the data is not encrypted",
    );
}

#[tokio::test]
async fn another_key_cannot_read_the_table() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let keyring = Keyring::create(PASSPHRASE).expect("create");
    {
        let store = LanceStore::new(root.path()).encrypted(&keyring);
        store
            .create_table("documents", &uncompressed())
            .await
            .expect("create");
        store.append("documents", &rows()).await.expect("append");
    }

    // A different database's keyring — same passphrase, different data key, which is the whole point of
    // generating the data key rather than deriving it.
    let other = Keyring::create(PASSPHRASE).expect("create");
    let result = LanceStore::new(root.path())
        .encrypted(&other)
        .scan("documents")
        .await;

    assert!(
        result.is_err(),
        "another key read the table, so the data is not actually protected by this one",
    );
}

#[tokio::test]
async fn a_plaintext_table_is_not_read_as_encrypted() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    {
        let store = LanceStore::new(root.path());
        store
            .create_table("documents", &uncompressed())
            .await
            .expect("create");
        store.append("documents", &rows()).await.expect("append");
    }

    let keyring = Keyring::create(PASSPHRASE).expect("create");
    let result = LanceStore::new(root.path())
        .encrypted(&keyring)
        .scan("documents")
        .await;

    assert!(
        result.is_err(),
        "an unencrypted table was read through the encrypting layer, so the layer interprets \
         arbitrary bytes as blocks instead of refusing",
    );
}

#[tokio::test]
async fn a_store_rooted_at_a_relative_path_still_works() {
    // The Windows failure was a path being formatted into a URI instead of converted into one. A
    // relative root exercises the same conversion — it has to be absolutised before it can be a URL —
    // and it is a legitimate thing for a caller to ask for.
    //
    // Changing the working directory is process-global, which is safe here only because nextest runs
    // each test in its own process. Under plain `cargo test` this would race every other test.
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let previous = std::env::current_dir().expect("a working directory");
    std::env::set_current_dir(root.path()).expect("the temporary directory is enterable");

    let keyring = Keyring::create(PASSPHRASE).expect("create");
    let store = LanceStore::new("./relative-root").encrypted(&keyring);
    let outcome = async {
        store.create_table("documents", &uncompressed()).await?;
        store.append("documents", &rows()).await?;
        store.scan("documents").await
    }
    .await;

    std::env::set_current_dir(previous).expect("the previous directory is still there");
    let read_back = outcome.expect("a relative root must work, absolutised into a URI");
    assert_eq!(
        read_back.iter().map(RecordBatch::num_rows).sum::<usize>(),
        2_000,
        "a store rooted at a relative path lost rows",
    );
}
