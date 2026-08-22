//! Every failure comes back as a value in one type, and none of them takes the process down with it.
//!
//! One test per case the criterion names — malformed input, a missing file, a permission problem, a
//! failed decryption — so that a CI line says which of the four broke. The malformed-input cases share
//! one test because they share a store and an assertion shape, and `assert_refused`'s `what` keeps them
//! apart in the failure message.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{
    Assignment, Error, InvalidZstdLevel, Keyring, KeyringError, LanceStore, Predicate,
    SchemaMismatch, StorageError, TableDefinition, TableSchema, TableStore, ZstdLevel,
};

fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("body", DataType::Utf8, false),
    ])))
}

fn rows() -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from(vec![1_i64, 2, 3]));
    let body: ArrayRef = Arc::new(StringArray::from(vec!["eins", "zwei", "drei"]));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("body", body, false)])
        .expect("batch")
}

/// Assert that an operation was refused by the layer below, naming the table it was about.
///
/// Deliberately not `is_err()`: that assertion survives the guard being deleted and the wrong error
/// being returned in its place. Matching the variant and the table is what makes it falsifiable, and
/// `what` keeps the failure line readable when one of several cases in a test breaks.
#[track_caller]
fn assert_refused<T: std::fmt::Debug>(outcome: Result<T, Error>, table: &str, what: &str) {
    let error = match outcome {
        Err(error) => error,
        Ok(value) => panic!("{what}: expected a refusal naming `{table}`, got Ok({value:?})"),
    };
    match &error {
        Error::Storage(StorageError::Backend { table: named, .. }) => {
            assert_eq!(
                named.as_str(),
                table,
                "{what}: the error must name the table it was about",
            );
            assert!(
                std::error::Error::source(&error).is_some(),
                "{what}: the cause below must stay reachable, or the message is all a caller gets",
            );
        }
        other => panic!("{what}: expected a refusal naming `{table}`, got {other:?}"),
    }
}

/// A panic here fails the test by itself, which is half the assertion — the other half is that the
/// error names the case, because "something went wrong" is not a usable answer for a caller.
#[tokio::test]
async fn no_public_call_panics_on_bad_input() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("write role");
    store
        .create_table("documents", &documents())
        .await
        .expect("create");
    store.append("documents", &rows()).await.expect("append");

    assert_refused(
        store.scan("absent").await,
        "absent",
        "a table that does not exist",
    );
    assert_refused(
        store
            .delete("documents", &Predicate::new("kein_feld = 1"))
            .await,
        "documents",
        "a predicate naming a column that is not there",
    );
    assert_refused(
        store
            .delete("documents", &Predicate::new("))) not an expression ((("))
            .await,
        "documents",
        "text that is not an expression at all",
    );
    assert_refused(
        store
            .update(
                "documents",
                &Predicate::new("id = 1"),
                &[Assignment::new("body", "))(")],
            )
            .await,
        "documents",
        "an assignment whose right-hand side is nonsense",
    );
    assert_refused(
        store
            .row_ids("documents", &Predicate::new("kein_feld = 1"))
            .await,
        "documents",
        "asking for row ids under a predicate naming a column that is not there",
    );
    assert_refused(
        store
            .row_ids_measured("documents", &Predicate::new("))) not an expression ((("))
            .await,
        "documents",
        "measuring a query that is not an expression",
    );
    assert_refused(
        store.create_index("documents", "kein_feld").await,
        "documents",
        "indexing a column the table does not have",
    );
    assert_refused(
        store
            .row_ids_all("documents", &[Predicate::new("kein_feld = 1")])
            .await,
        "documents",
        "combining a predicate naming a column that is not there",
    );
    assert_refused(
        store
            .scan_ordered(
                "documents",
                &Predicate::new("id = 1"),
                "kein_feld",
                euledb_storage::Order::Ascending,
            )
            .await,
        "documents",
        "ordering by a column the table does not have",
    );

    let wrong: ArrayRef = Arc::new(StringArray::from(vec!["nicht", "eine", "zahl"]));
    let mismatched =
        RecordBatch::try_from_iter_with_nullable([("id", wrong, false)]).expect("batch");
    assert!(
        matches!(
            documents().schema().validate(&mismatched),
            Err(Error::Schema(_)),
        ),
        "a batch that is not the declared table must come back as a schema mismatch",
    );

    assert!(
        matches!(
            ZstdLevel::new(0),
            Err(Error::Compression(InvalidZstdLevel { given: 0 })),
        ),
        "a level the compressor does not define must name the value it refused",
    );

    // Two ways for a keyfile to be wrong, and they are worth telling apart: the first byte is a format
    // version, so text fails on the version before anything else is even looked at.
    assert!(
        matches!(
            Keyring::open(b"nicht mal in der Naehe eines Keyfiles", "irgendwas"),
            Err(Error::Keyring(KeyringError::UnsupportedVersion { found })) if found == b'n',
        ),
        "text read as a keyfile must be refused on its version byte, naming what it found",
    );
    // The literal 2 rather than the crate's constant, on purpose: this pins the on-disk keyfile version
    // from the outside, so bumping it silently fails here instead of shipping.
    assert!(
        matches!(
            Keyring::open(&[2], "irgendwas"),
            Err(Error::Keyring(KeyringError::MalformedKeyfile { .. })),
        ),
        "the right version with nothing behind it must be refused as malformed",
    );
}

#[tokio::test]
async fn a_missing_file_is_an_error_and_not_a_panic() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("write role");
    store
        .create_table("documents", &documents())
        .await
        .expect("create");

    // Absent table in a database that exists, and an absent database entirely. Both are a caller
    // reading a name that is not there — after a rename, or after opening yesterday's path.
    assert_refused(
        store.scan("absent").await,
        "absent",
        "a table the database does not hold",
    );
    assert_refused(
        LanceStore::new(root.path().join("nicht-da"))
            .scan("documents")
            .await,
        "documents",
        "a database directory that was never created",
    );
}

#[tokio::test]
async fn a_failed_decryption_is_an_error_and_not_a_panic() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let keyring = Keyring::create("korrektes-pferd-batterie-heftklammer").expect("keyring");
    {
        let store = LanceStore::open_for_writing(root.path())
            .expect("write role")
            .encrypted(&keyring);
        store
            .create_table("documents", &documents())
            .await
            .expect("create");
        store.append("documents", &rows()).await.expect("append");
    }

    // Same passphrase, different keyring: the data keys are random, so this is a reader holding keys
    // that do not open this database — the realistic shape of a failed decryption.
    let stranger = Keyring::create("korrektes-pferd-batterie-heftklammer").expect("keyring");
    assert_refused(
        LanceStore::new(root.path())
            .encrypted(&stranger)
            .scan("documents")
            .await,
        "documents",
        "keys that do not open this database",
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_file_that_cannot_be_read_is_an_error_and_not_a_panic() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().expect("a temporary directory is available");
    {
        let store = LanceStore::open_for_writing(root.path()).expect("write role");
        store
            .create_table("documents", &documents())
            .await
            .expect("create");
        store.append("documents", &rows()).await.expect("append");
    }

    // Take the read bit off everything under the table. Unix only: Windows permissions do not map onto
    // this, and pretending they do would make the test lie on one platform.
    let mut stack = vec![root.path().to_path_buf()];
    let mut touched = Vec::new();
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path).into_iter().flatten().flatten() {
            let child = entry.path();
            if child.is_dir() {
                stack.push(child);
            } else {
                let _ = std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o000));
                touched.push(child);
            }
        }
    }

    let outcome = LanceStore::new(root.path()).scan("documents").await;

    // Restored before asserting, so a failure does not leave an undeletable directory behind.
    for path in touched {
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    assert_refused(outcome, "documents", "a table whose files cannot be read");
}

/// One type, and it does not get in the way of reading the failure.
///
/// The claim is that wrapping is free at the surface: `Display` passes straight through to the failure
/// that actually happened, so a caller who only logs the error learns exactly what went wrong and never
/// reads a category label. Dropping an `#[error(transparent)]` breaks this and nothing else would
/// notice. `source()` is deliberately not asserted here — `transparent` forwards that too, so the
/// wrapper is invisible in the chain and the specific failure is recovered by matching the variant,
/// which is what the `match` in every test above does.
#[test]
fn the_one_type_renders_as_the_failure_it_carries() {
    let schema = SchemaMismatch::MissingColumn {
        column: "language".to_owned(),
    };
    let storage = StorageError::NothingToSet {
        table: "documents".to_owned(),
    };
    let keyring = KeyringError::WrongPassphrase;
    let level = InvalidZstdLevel { given: 99 };

    for (carried, wrapped) in [
        (schema.to_string(), Error::from(schema)),
        (storage.to_string(), Error::from(storage)),
        (keyring.to_string(), Error::from(keyring)),
        (level.to_string(), Error::from(level)),
    ] {
        assert!(
            !carried.is_empty(),
            "a failure that renders as nothing tells a caller nothing",
        );
        assert_eq!(
            wrapped.to_string(),
            carried,
            "the one type must render as the failure it carries, not as a category name",
        );
    }
}
