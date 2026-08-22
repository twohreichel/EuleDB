//! A gated database is read-only until a signed token says otherwise.

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
    Error, Keyring, LanceStore, Scope, StorageError, TableDefinition, TableSchema, TableStore,
};

fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
    ])))
}

fn rows() -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(vec![4218_i64, 4219]));
    let title: ArrayRef = Arc::new(StringArray::from(vec![
        "Grundsatzurteil zur Vorratsdatenspeicherung",
        "Rapport sur la souveraineté numérique",
    ]));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("title", title, false)])
        .expect("the batch matches the declared schema")
}

/// A database with a table already in it, and the keyring that is its authority.
async fn populated() -> (tempfile::TempDir, Keyring) {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let keyring = Keyring::create("korrektes-pferd-batterie-heftklammer").expect("keyring");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");
    drop(store);
    (root, keyring)
}

#[tokio::test]
async fn a_gated_database_reads_with_a_read_token_and_refuses_to_write() {
    let (root, keyring) = populated().await;

    let gated = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .gated(&keyring, vec![keyring.grant("documents", Scope::Read)]);

    let read: usize = gated
        .scan("documents")
        .await
        .expect("a read token permits reading")
        .iter()
        .map(RecordBatch::num_rows)
        .sum();
    assert_eq!(read, 2, "a read token must permit exactly reading");

    // The write role is held — this handle could write if nothing else stopped it. The token is what
    // stops it, which is the difference between the concurrency rule and the authorisation rule.
    let refusal = gated
        .append("documents", &rows())
        .await
        .expect_err("a read token must not permit writing");
    assert!(
        matches!(
            &refusal,
            Error::Storage(StorageError::NotPermitted { scope, .. }) if *scope == Scope::Write
        ),
        "the refusal must name the scope that was missing: {refusal:?}",
    );
}

/// The half of AC-28 that is easy to get wrong: a refusal must not leak what the database holds.
#[tokio::test]
async fn a_refusal_does_not_reveal_whether_the_table_exists() {
    let (root, keyring) = populated().await;
    let gated = LanceStore::new(root.path()).gated(&keyring, Vec::new());

    let present = gated
        .scan("documents")
        .await
        .expect_err("no read token, so an existing table is refused");
    let absent = gated
        .scan("gibt-es-nicht")
        .await
        .expect_err("no read token, so a table that is not there is refused too");

    // Same variant, same scope, and the only difference in the message is the name the caller supplied.
    // Anything else — a different variant, a different wording, a longer chain — would be an oracle for
    // enumerating a database a caller is not allowed to read.
    assert!(
        matches!(&present, Error::Storage(StorageError::NotPermitted { scope, .. }) if *scope == Scope::Read),
        "an existing table must be refused for the missing scope: {present:?}",
    );
    assert_eq!(
        present.to_string().replace("documents", "TABLE"),
        absent.to_string().replace("gibt-es-nicht", "TABLE"),
        "the two refusals must be the same message but for the name the caller already knew",
    );
    assert_eq!(
        std::error::Error::source(&present).is_some(),
        std::error::Error::source(&absent).is_some(),
        "one refusal must not carry a cause the other does not — the chain is an oracle too",
    );
}

#[tokio::test]
async fn a_token_from_another_authority_is_not_honoured() {
    let (root, keyring) = populated().await;
    // A second keyring, same passphrase. Its key-encryption key is derived over a different random
    // salt, so its token key differs and its tags cannot verify here.
    let stranger = Keyring::create("korrektes-pferd-batterie-heftklammer").expect("keyring");

    let gated = LanceStore::new(root.path())
        .gated(&keyring, vec![stranger.grant("documents", Scope::Read)]);

    let refusal = gated
        .scan("documents")
        .await
        .expect_err("a token another authority signed must not be honoured");
    assert!(
        matches!(&refusal, Error::Storage(StorageError::NotPermitted { scope, .. }) if *scope == Scope::Read),
        "a token that does not verify must be refused as though it were absent: {refusal:?}",
    );
}

#[tokio::test]
async fn a_token_for_one_table_does_not_open_another() {
    let (root, keyring) = populated().await;
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("berichte", &documents())
        .await
        .expect("a second table is declared");
    drop(store);

    let gated =
        LanceStore::new(root.path()).gated(&keyring, vec![keyring.grant("documents", Scope::Read)]);

    gated
        .scan("documents")
        .await
        .expect("the table the token names is readable");
    let refusal = gated
        .scan("berichte")
        .await
        .expect_err("a token naming one table must not open another");
    assert!(
        matches!(
            &refusal,
            Error::Storage(StorageError::NotPermitted { table, .. }) if table == "berichte"
        ),
        "the refusal must be about the table that was asked for: {refusal:?}",
    );
}

/// Scopes are independent, and that is a decision rather than an oversight.
///
/// A write token does not permit reading. Explicit is worth the extra grant in an authorisation model:
/// an implicit escalation is exactly the convenience that becomes a finding.
#[tokio::test]
async fn a_write_token_does_not_permit_reading() {
    let (root, keyring) = populated().await;
    let gated = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .gated(&keyring, vec![keyring.grant("documents", Scope::Write)]);

    gated
        .append("documents", &rows())
        .await
        .expect("the write token permits writing");
    let refusal = gated
        .scan("documents")
        .await
        .expect_err("a write token must not permit reading");
    assert!(
        matches!(&refusal, Error::Storage(StorageError::NotPermitted { scope, .. }) if *scope == Scope::Read),
        "reading must need its own token: {refusal:?}",
    );
}

/// Declaring and dropping a table need the schema scope, not the write scope.
#[tokio::test]
async fn changing_the_shape_of_a_database_needs_the_schema_scope() {
    let (root, keyring) = populated().await;
    let gated = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .gated(&keyring, vec![keyring.grant("neu", Scope::Write)]);

    for outcome in [
        gated.create_table("neu", &documents()).await,
        gated.drop_table("neu").await,
        gated.create_index("neu", "id").await,
    ] {
        let refusal = outcome.expect_err("a write token must not reshape a database");
        assert!(
            matches!(&refusal, Error::Storage(StorageError::NotPermitted { scope, .. }) if *scope == Scope::Schema),
            "reshaping must need the schema scope: {refusal:?}",
        );
    }
}

/// The authority's own handle is not gated, and everything that worked still works.
#[tokio::test]
async fn an_ungated_handle_is_unchanged() {
    let (root, _keyring) = populated().await;
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");

    store
        .append("documents", &rows())
        .await
        .expect("an ungated handle writes as it always did");
    let read: usize = store
        .scan("documents")
        .await
        .expect("and reads")
        .iter()
        .map(RecordBatch::num_rows)
        .sum();
    assert_eq!(read, 4, "two rows were there and two more were added");
}
