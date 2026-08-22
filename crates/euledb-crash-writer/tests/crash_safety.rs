//! Kill the writer mid-write, reopen, and see what survived.
//!
//! The criterion asks for this to be proven by killing the writer at several points rather than by
//! argument, so the test starts a real process, lets it write, and terminates it without warning.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::io::{BufRead as _, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use arrow_array::RecordBatch;
use euledb_storage::{LanceStore, TableStore};

/// Rows per append. Every committed append adds exactly this many, so a surviving row count that is not
/// a multiple of it means a commit was applied in part.
const BATCH: usize = 250;

/// Start the writer and wait until it says the table exists, so the timings below measure writing rather
/// than start-up.
fn start_writer(root: &std::path::Path) -> std::process::Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_euledb-crash-writer"))
        .arg(root)
        .arg(BATCH.to_string())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the crash-writer fixture must be runnable");

    let stdout = child.stdout.take().expect("stdout was piped");
    let mut lines = BufReader::new(stdout).lines();
    let first = lines
        .next()
        .and_then(Result::ok)
        .expect("the writer must announce that the table exists");
    assert_eq!(
        first, "ready",
        "the writer said something unexpected: {first}"
    );
    child
}

/// Every row the database still holds, or the error it refuses to open with.
async fn surviving_rows(root: &std::path::Path) -> Result<usize, String> {
    LanceStore::new(root)
        .scan("counters")
        .await
        .map(|batches| batches.iter().map(RecordBatch::num_rows).sum())
        .map_err(|err| err.to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_writer_killed_mid_write_leaves_a_whole_number_of_commits() {
    // Several points, not one. The delays land in different phases — before the first append completes,
    // between commits, and in the middle of a later one — and the invariant has to hold at every one of
    // them, including the case where nothing was committed yet.
    let mut observed = Vec::new();
    for delay in [15_u64, 45, 90, 180, 350] {
        let root = tempfile::tempdir().expect("a temporary directory is available");
        let mut writer = start_writer(root.path());
        std::thread::sleep(Duration::from_millis(delay));

        writer.kill().expect("the writer must be killable");
        let status = writer.wait().expect("the killed writer must be reapable");
        assert!(
            !status.success(),
            "the writer exited on its own after {delay} ms, so nothing was interrupted",
        );

        let rows = surviving_rows(root.path()).await.unwrap_or_else(|err| {
            panic!("after a kill at {delay} ms the database is unreadable: {err}")
        });

        assert_eq!(
            rows % BATCH,
            0,
            "after a kill at {delay} ms the database holds {rows} rows, which is not a whole number of \
             appends — a commit was applied in part",
        );
        observed.push((delay, rows));
    }

    // Without this the test is vacuous: zero rows is a whole number of appends too, so a writer that
    // never managed to commit anything would satisfy every assertion above. At least one of the longer
    // delays has to have got some data in, or the kills were not interrupting a write at all.
    assert!(
        observed.iter().any(|(_, rows)| *rows > 0),
        "no run committed a single row, so nothing was interrupted mid-write: {observed:?}",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_killed_writer_does_not_lock_the_database_out() {
    // The reason the write role is an advisory lock on an open handle rather than a marker file: the
    // operating system releases it when the process dies, however it dies. A marker would outlive the
    // crash and the first thing anyone would learn is how to delete it.
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let mut writer = start_writer(root.path());
    std::thread::sleep(Duration::from_millis(60));
    writer.kill().expect("kill");
    writer.wait().expect("reap");

    let recovered = LanceStore::open_for_writing(root.path())
        .expect("a database whose writer was killed must be writable again");
    let rows = recovered
        .scan("counters")
        .await
        .expect("and readable")
        .iter()
        .map(RecordBatch::num_rows)
        .sum::<usize>();
    assert_eq!(
        rows % BATCH,
        0,
        "the recovered database holds {rows} rows, which is not a whole number of appends",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_killed_writer_loses_no_commit_that_was_reported_complete() {
    // Not merely "the state is consistent" — it has to be the state before or after a write, and a
    // write that completed must not be among the losses. Two kills of the same database: whatever
    // survived the first must still be there after the second.
    let root = tempfile::tempdir().expect("a temporary directory is available");

    let mut first = start_writer(root.path());
    std::thread::sleep(Duration::from_millis(200));
    first.kill().expect("kill");
    first.wait().expect("reap");
    let after_first = surviving_rows(root.path())
        .await
        .expect("readable after the first kill");

    let mut second = start_writer(root.path());
    std::thread::sleep(Duration::from_millis(120));
    second.kill().expect("kill");
    second.wait().expect("reap");
    let after_second = surviving_rows(root.path())
        .await
        .expect("readable after the second kill");

    assert!(
        after_second >= after_first,
        "the database held {after_first} rows and now holds {after_second}: a completed commit was lost",
    );
    assert_eq!(
        after_second % BATCH,
        0,
        "the database holds {after_second} rows, which is not a whole number of appends",
    );
}
