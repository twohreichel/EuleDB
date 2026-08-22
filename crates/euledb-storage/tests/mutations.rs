//! Updating and deleting rows: exactly the matching ones, and a delete that announces itself first.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{
    Assignment, Keyring, LanceStore, Predicate, StorageError, TableDefinition, TableSchema,
    TableStore,
};

fn documents() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("language", DataType::Utf8, false),
        Field::new("body", DataType::Utf8, false),
    ])))
}

/// Nine rows across three languages, so a predicate has both matching and non-matching rows to
/// distinguish and a count that cannot be reached by accident.
fn rows() -> RecordBatch {
    let languages = ["de", "fr", "pl"];
    let id: ArrayRef = Arc::new(Int64Array::from((0..9_i64).collect::<Vec<i64>>()));
    let language: ArrayRef = Arc::new(StringArray::from(
        (0..9_usize)
            .map(|row| languages[row % 3])
            .collect::<Vec<&str>>(),
    ));
    let body: ArrayRef = Arc::new(StringArray::from(
        (0..9)
            .map(|row| format!("Randnummer {row}"))
            .collect::<Vec<String>>(),
    ));
    RecordBatch::try_from_iter_with_nullable([
        ("id", id, false),
        ("language", language, false),
        ("body", body, false),
    ])
    .expect("the batch matches the declared schema")
}

/// Every row as (id, language, body), sorted by id, so a comparison reads as a table.
fn contents(batches: &[RecordBatch]) -> Vec<(i64, String, String)> {
    let mut out: Vec<(i64, String, String)> = batches
        .iter()
        .flat_map(|batch| {
            let ids = batch
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("column 0 is the Int64 id");
            let languages = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("column 1 is the language");
            let bodies = batch
                .column(2)
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("column 2 is the body");
            (0..batch.num_rows())
                .map(|row| {
                    (
                        ids.value(row),
                        languages.value(row).to_owned(),
                        bodies.value(row).to_owned(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    out.sort();
    out
}

async fn seeded(root: &std::path::Path) -> LanceStore {
    let store = LanceStore::open_for_writing(root).expect("taking the write role must succeed");
    store
        .create_table("documents", &documents())
        .await
        .expect("create");
    store.append("documents", &rows()).await.expect("append");
    store
}

#[tokio::test]
async fn an_update_changes_the_matching_rows_and_only_those() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let before = {
        let store = seeded(root.path()).await;
        let updated = store
            .update(
                "documents",
                &Predicate::new("language = 'fr'"),
                &[Assignment::new("body", "'redigé'")],
            )
            .await
            .expect("updating must succeed");
        assert_eq!(updated.rows, 3, "three of nine rows are French");
        contents(&store.scan("documents").await.expect("scan"))
    };

    // Reopened, because the criterion asks for the values to be there after the handle is dropped.
    let after = contents(
        &LanceStore::new(root.path())
            .scan("documents")
            .await
            .expect("scan after reopen"),
    );
    assert_eq!(before, after, "the update did not survive a reopen");

    for (id, language, body) in &after {
        if language == "fr" {
            assert_eq!(body, "redigé", "row {id} is French and was not updated");
        } else {
            assert_eq!(
                body,
                &format!("Randnummer {id}"),
                "row {id} is not French and was changed anyway",
            );
        }
    }
}

#[tokio::test]
async fn a_delete_removes_exactly_the_matching_rows_and_reports_how_many() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = seeded(root.path()).await;

    let deleted = store
        .delete("documents", &Predicate::new("language = 'pl'"))
        .await
        .expect("deleting must succeed");

    assert_eq!(deleted.rows, 3, "three of nine rows are Polish");
    let left = contents(&store.scan("documents").await.expect("scan"));
    assert_eq!(left.len(), 6, "six rows should be left");
    assert!(
        left.iter().all(|(_, language, _)| language != "pl"),
        "a Polish row survived the delete",
    );
    assert!(
        left.iter().any(|(_, language, _)| language == "de"),
        "the delete took rows it was not asked for",
    );
}

#[tokio::test]
async fn a_delete_that_matches_nothing_reports_nothing_and_changes_nothing() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = seeded(root.path()).await;
    let before = contents(&store.scan("documents").await.expect("scan"));

    let deleted = store
        .delete("documents", &Predicate::new("language = 'is'"))
        .await
        .expect("a predicate matching nothing is not an error");

    assert_eq!(
        deleted.rows, 0,
        "rows were reported deleted that do not exist"
    );
    assert_eq!(
        contents(&store.scan("documents").await.expect("scan")),
        before,
        "a delete matching nothing changed the table",
    );
}

#[tokio::test]
async fn a_predicate_naming_a_column_that_does_not_exist_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = seeded(root.path()).await;
    let before = contents(&store.scan("documents").await.expect("scan"));

    let outcome = store
        .delete("documents", &Predicate::new("dialect = 'de'"))
        .await;

    assert!(
        outcome.is_err(),
        "a predicate naming an unknown column was accepted, which is how a delete matches the wrong \
         rows or none at all without anyone noticing",
    );
    assert_eq!(
        contents(&store.scan("documents").await.expect("scan")),
        before,
        "the refused delete still changed the table",
    );
}

#[tokio::test]
async fn the_count_a_delete_reports_is_the_number_of_rows_that_left() {
    // The reported count checked against something that does not come from the same call: the rows
    // actually present before and after. A count taken from the delete's own return value and compared
    // to itself would prove nothing.
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = seeded(root.path()).await;
    let before = contents(&store.scan("documents").await.expect("scan")).len();

    let deleted = store
        .delete("documents", &Predicate::new("id < 4"))
        .await
        .expect("delete");
    let after = contents(&store.scan("documents").await.expect("scan")).len();

    assert_eq!(deleted.rows, 4, "ids 0 through 3 are four rows");
    assert_eq!(
        u64::try_from(before - after).unwrap_or(u64::MAX),
        deleted.rows,
        "the delete reported {} rows but {} left the table",
        deleted.rows,
        before - after,
    );
}

#[tokio::test]
async fn an_update_with_nothing_to_set_is_refused() {
    // An update that changes nothing is a mistake at the call site. Reporting success would hide it, and
    // the caller would go looking for the change somewhere else.
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = seeded(root.path()).await;

    let error = store
        .update("documents", &Predicate::new("id < 4"), &[])
        .await
        .expect_err("an update with no assignments must be refused");

    // The specific error, not merely "it failed". The layer below refuses this too, so asserting only
    // is_err() passed with the guard removed — and the caller would get a message about a query plan
    // instead of one about their own call.
    assert!(
        matches!(error, StorageError::NothingToSet { ref table } if table == "documents"),
        "the refusal did not name the mistake, so the caller learns nothing: {error:?}",
    );
}

/// Collects every event's rendered fields, so a test can assert what was announced rather than trusting
/// that something was.
#[derive(Clone, Default)]
struct Recorder(Arc<std::sync::Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Recorder {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        struct Fields(String);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push_str(&format!(" {}={value:?}", field.name()));
            }
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                self.0.push_str(&format!(" {}={value}", field.name()));
            }
            fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
                self.0.push_str(&format!(" {}={value}", field.name()));
            }
        }
        let mut fields = Fields(format!("{}:", event.metadata().level()));
        event.record(&mut fields);
        if let Ok(mut recorded) = self.0.lock() {
            recorded.push(fields.0);
        }
    }
}

#[tokio::test]
async fn a_delete_announces_the_count_and_the_predicate_before_it_runs() {
    // The unusual half of the criterion, and the half that is a promise unless something checks it: a
    // delete broader than intended has to be visible in the log rather than inferred later from rows
    // that are gone.
    use tracing_subscriber::layer::SubscriberExt as _;

    let recorder = Recorder::default();
    let subscriber = tracing_subscriber::registry().with(recorder.clone());
    let guard = tracing::subscriber::set_default(subscriber);

    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = seeded(root.path()).await;
    store
        .delete("documents", &Predicate::new("language = 'de'"))
        .await
        .expect("delete");
    drop(guard);

    let recorded = recorder
        .0
        .lock()
        .expect("the recorder is not poisoned")
        .clone();
    let announcement = recorded
        .iter()
        .find(|line| line.contains("deleting rows"))
        .unwrap_or_else(|| panic!("no delete was announced at all. Recorded: {recorded:#?}"));

    for expected in ["WARN", "rows=3", "language = 'de'", "documents"] {
        assert!(
            announcement.contains(expected),
            "the announcement is missing {expected:?}, so an operator cannot tell what was about to \
             be removed: {announcement}",
        );
    }
}

#[tokio::test]
async fn update_and_delete_work_on_an_encrypted_table() {
    // Mutations rewrite fragments and commit a new manifest, so they exercise the encrypting layer's
    // write path as well as its read path. Nothing here should know the difference — and if it does,
    // this is where it shows.
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let keyring = Keyring::create("korrektes-pferd-batterie-heftklammer").expect("keyring");
    let store = LanceStore::open_for_writing(root.path())
        .expect("taking the write role must succeed")
        .encrypted(&keyring);
    store
        .create_table("documents", &documents())
        .await
        .expect("create");
    store.append("documents", &rows()).await.expect("append");

    let updated = store
        .update(
            "documents",
            &Predicate::new("language = 'de'"),
            &[Assignment::new("body", "'Beschluss'")],
        )
        .await
        .expect("updating an encrypted table must succeed");
    assert_eq!(updated.rows, 3, "three of nine rows are German");

    let deleted = store
        .delete("documents", &Predicate::new("language = 'fr'"))
        .await
        .expect("deleting from an encrypted table must succeed");
    assert_eq!(deleted.rows, 3, "three of nine rows are French");

    let left = contents(&store.scan("documents").await.expect("scan"));
    assert_eq!(left.len(), 6, "six rows should be left");
    for (id, language, body) in &left {
        match language.as_str() {
            "de" => assert_eq!(body, "Beschluss", "row {id} is German and was not updated"),
            "pl" => assert_eq!(
                body,
                &format!("Randnummer {id}"),
                "row {id} was changed anyway"
            ),
            other => panic!("row {id} has language {other}, which should have been deleted"),
        }
    }
}
