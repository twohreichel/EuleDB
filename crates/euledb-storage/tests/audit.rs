//! Every operation leaves a record, and the records form a chain.

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
    AuditLog, AuditRecord, LanceStore, Predicate, TableDefinition, TableSchema, TableStore,
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

#[tokio::test]
async fn every_operation_leaves_a_record_and_the_records_chain() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .audited();

    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");
    store
        .delete("documents", &Predicate::new("id = 4219"))
        .await
        .expect("one row leaves");
    store.scan("documents").await.expect("and the rest reads");

    let log = AuditLog::open(root.path());
    let records = log.records().expect("the log parses");

    assert_eq!(
        records.len(),
        4,
        "four operations, four records — a read is an operation too",
    );
    assert_eq!(
        records.iter().map(|r| r.sequence()).collect::<Vec<u64>>(),
        vec![0, 1, 2, 3],
        "sequence numbers must run without a gap, which is what makes a removal visible",
    );

    // The delete removed one row and the record has to say so — hand-read, not taken from the code.
    let delete = records
        .iter()
        .find(|r| r.query().contains("delete"))
        .expect("the delete is in the log");
    assert_eq!(delete.rows(), 1, "one row matched the predicate and left");
    assert!(
        delete.query().contains("id = 4219"),
        "the record must say what was asked, not merely that something was: {:?}",
        delete.query(),
    );

    // Every link names its predecessor, and the first names nothing — that is what anchors the chain.
    // Verifying it and reporting a break is the next criterion; this one only claims the links exist.
    assert_eq!(
        records[0].previous(),
        &[0_u8; 32],
        "the first record must anchor the chain rather than point at something",
    );
    for pair in records.windows(2) {
        assert_eq!(
            pair[1].previous(),
            pair[0].hash(),
            "record {} must name record {} as its predecessor",
            pair[1].sequence(),
            pair[0].sequence(),
        );
    }
}

/// The rows a record describes must not be in it.
///
/// An audit log that copies the rows it describes is a second copy of the database, and this one is not
/// encrypted. The row values are deliberately distinctive so their absence is a real assertion.
#[tokio::test]
async fn the_log_does_not_copy_the_rows_it_describes() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .audited();
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    let raw = std::fs::read_to_string(root.path().join(".euledb-audit.log"))
        .expect("the log is a readable file");
    for secret in [
        "Grundsatzurteil zur Vorratsdatenspeicherung",
        "Rapport sur la souveraineté numérique",
    ] {
        assert!(
            !raw.contains(secret),
            "the log must describe the write, not reproduce it: {secret:?} is in the log",
        );
    }
}

#[tokio::test]
async fn an_unaudited_store_writes_no_log_at_all() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path()).expect("the write role is free");
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    assert!(
        !root.path().join(".euledb-audit.log").exists(),
        "auditing is a tunable, and off must mean no file — a database on read-only media stays readable",
    );
}

/// The claim that makes AC-29 compatible with AC-70: many readers can each record their read.
///
/// A recorded read is a write, so if the log took the *database's* write lock the second reader would be
/// refused outright. That is what this establishes.
///
/// It does **not** establish that the log's own lock works: removing the lock leaves this test green,
/// because twelve readers doing real database work do not collide. The lock is tested under
/// manufactured contention further down — this comment says so rather than letting the next reader
/// assume otherwise.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_readers_all_append_and_the_chain_stays_gapless() {
    /// Enough readers that an unlocked append would collide, few enough to stay fast.
    const READERS: u64 = 12;

    let root = tempfile::tempdir().expect("a temporary directory is available");
    {
        let writer = LanceStore::open_for_writing(root.path()).expect("the write role is free");
        writer
            .create_table("documents", &documents())
            .await
            .expect("the table is declared");
        writer
            .append("documents", &rows())
            .await
            .expect("rows land");
    }
    // The setup handle was not audited, so the log starts empty and every record below is a reader's.
    let path = root.path().to_path_buf();

    let readers = (0..READERS).map(|_| {
        let path = path.clone();
        tokio::spawn(async move {
            LanceStore::new(&path)
                .audited()
                .scan("documents")
                .await
                .map(|batches| batches.len())
        })
    });
    for reader in readers {
        reader
            .await
            .expect("no reader panicked")
            .expect("every reader reads, and records that it did");
    }

    let records = AuditLog::open(root.path())
        .records()
        .expect("the log parses");
    assert_eq!(
        records.len(),
        usize::try_from(READERS).expect("fits"),
        "every reader must have left exactly one record",
    );

    let sequences: Vec<u64> = records.iter().map(AuditRecord::sequence).collect();
    assert_eq!(
        sequences,
        (0..READERS).collect::<Vec<u64>>(),
        "sequence numbers must be gapless and unique — a collision means two readers raced",
    );
    for pair in records.windows(2) {
        assert_eq!(
            pair[1].previous(),
            pair[0].hash(),
            "the chain must survive concurrent appends, not merely the sequence numbers",
        );
    }
}

/// A read through a read-only handle is recorded, which is the surprising part of the decision.
#[tokio::test]
async fn a_read_only_handle_still_records_its_reads() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    {
        let writer = LanceStore::open_for_writing(root.path()).expect("the write role is free");
        writer
            .create_table("documents", &documents())
            .await
            .expect("the table is declared");
        writer
            .append("documents", &rows())
            .await
            .expect("rows land");
    }

    LanceStore::new(root.path())
        .audited()
        .scan("documents")
        .await
        .expect("a reader reads");

    let records = AuditLog::open(root.path())
        .records()
        .expect("the log parses");
    assert_eq!(records.len(), 1, "the read is the only operation recorded");
    assert!(
        records[0].query().contains("scan") && records[0].query().contains("documents"),
        "the record must say what was read: {:?}",
        records[0].query(),
    );
    assert_eq!(records[0].rows(), 2, "two rows were returned");
}

/// A tab or a newline in a predicate must not be able to forge a field boundary.
#[tokio::test]
async fn a_predicate_carrying_the_separator_cannot_forge_a_record() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::open_for_writing(root.path())
        .expect("the write role is free")
        .audited();
    store
        .create_table("documents", &documents())
        .await
        .expect("the table is declared");
    store.append("documents", &rows()).await.expect("rows land");

    // A predicate that is not valid SQL, but the log records what was ASKED — including the attempt.
    let hostile = "id = 1\t99\t0000\tforged\tforged\nid = 2";
    let _ = store.delete("documents", &Predicate::new(hostile)).await;

    let records = AuditLog::open(root.path())
        .records()
        .expect("the log parses even after a hostile predicate");
    let sequences: Vec<u64> = records.iter().map(AuditRecord::sequence).collect();
    assert_eq!(
        sequences,
        (0..sequences.len() as u64).collect::<Vec<u64>>(),
        "an escaped separator must not add a record: {records:#?}",
    );
    for pair in records.windows(2) {
        assert_eq!(pair[1].previous(), pair[0].hash(), "the chain must hold");
    }
}

/// The lock, tested where it can actually be broken.
///
/// The reader test above turned out **not** to exercise this: twelve readers doing real database work
/// never collided, so removing the lock left it green. Contention has to be manufactured — real OS
/// threads hammering the append path with nothing in between — or the claim is untested.
#[test]
fn appends_from_many_threads_produce_a_gapless_chain() {
    /// Threads and appends each. The product is what must come back, and it is large enough that an
    /// unlocked read-then-write loses at least one of them essentially every run.
    const THREADS: u64 = 8;
    const EACH: u64 = 12;

    let root = tempfile::tempdir().expect("a temporary directory is available");
    let log = AuditLog::open(root.path());

    std::thread::scope(|scope| {
        for thread in 0..THREADS {
            let log = log.clone();
            scope.spawn(move || {
                for round in 0..EACH {
                    log.append(&format!("scan `t{thread}`"), "", round)
                        .expect("an append under contention still succeeds");
                }
            });
        }
    });

    let records = log.records().expect("the log parses");
    let sequences: Vec<u64> = records.iter().map(AuditRecord::sequence).collect();
    assert_eq!(
        sequences,
        (0..THREADS * EACH).collect::<Vec<u64>>(),
        "every append must take its own place in the chain — a duplicate or a gap is a lost record",
    );
    for pair in records.windows(2) {
        assert_eq!(
            pair[1].previous(),
            pair[0].hash(),
            "and each link must name the one before it, under contention as well",
        );
    }
}

/// Tamper with one record in the middle and the log must name that link, not the first or the last.
///
/// An assertion that verification merely failed would pass with an off-by-one in the reported index —
/// and that index is the number an operator acts on.
#[test]
fn verification_names_the_first_broken_link() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let log = AuditLog::open(root.path());
    for round in 0..5 {
        log.append(&format!("scan `t{round}`"), "", round)
            .expect("an append succeeds");
    }
    log.verify().expect("a log nobody touched must verify");

    // Rewrite the row count of record 2. Its own hash no longer matches its content, and every link
    // after it inherits the break.
    let path = root.path().join(".euledb-audit.log");
    let raw = std::fs::read_to_string(&path).expect("the log is readable");
    let tampered: Vec<String> = raw
        .lines()
        .map(|line| {
            let mut fields: Vec<&str> = line.split('\t').collect();
            if fields.first() == Some(&"2") {
                fields[3] = "9999";
            }
            fields.join("\t")
        })
        .collect();
    std::fs::write(&path, tampered.join("\n") + "\n").expect("the log is writable");

    let broken = log.verify().expect_err("a tampered log must not verify");
    assert!(
        matches!(&broken, euledb_storage::AuditError::BrokenChain { at } if *at == 2),
        "the report must name link 2 — not the first, not the last: {broken:?}",
    );
}

#[test]
fn a_removed_record_breaks_the_chain_at_the_gap() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let log = AuditLog::open(root.path());
    for round in 0..5 {
        log.append(&format!("scan `t{round}`"), "", round)
            .expect("an append succeeds");
    }

    // Delete record 3 entirely — the tidiest way to hide an operation, and the one a chain exists for.
    let path = root.path().join(".euledb-audit.log");
    let raw = std::fs::read_to_string(&path).expect("the log is readable");
    let kept: Vec<&str> = raw
        .lines()
        .filter(|line| !line.starts_with("3\t"))
        .collect();
    std::fs::write(&path, kept.join("\n") + "\n").expect("the log is writable");

    let broken = log
        .verify()
        .expect_err("a log with a record removed must not verify");
    assert!(
        matches!(&broken, euledb_storage::AuditError::BrokenChain { at } if *at == 4),
        "record 4 is the first whose predecessor is missing: {broken:?}",
    );
}

/// A broken chain fails closed: nothing more may be appended until someone says so explicitly.
///
/// A log that keeps accepting entries after it has been tampered with is worse than no log, because it
/// still looks trustworthy.
#[test]
fn a_broken_chain_refuses_further_appends_until_it_is_re_anchored() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let log = AuditLog::open(root.path());
    for round in 0..3 {
        log.append(&format!("scan `t{round}`"), "", round)
            .expect("an append succeeds");
    }

    let path = root.path().join(".euledb-audit.log");
    let raw = std::fs::read_to_string(&path).expect("the log is readable");
    let kept: Vec<&str> = raw
        .lines()
        .filter(|line| !line.starts_with("1\t"))
        .collect();
    std::fs::write(&path, kept.join("\n") + "\n").expect("the log is writable");

    let refused = log
        .append("scan `after`", "", 0)
        .expect_err("a broken chain must refuse an append");
    assert!(
        matches!(&refused, euledb_storage::AuditError::BrokenChain { .. }),
        "the refusal must be the break itself, not some other failure: {refused:?}",
    );

    // Re-anchoring is explicit, and it records the break it anchors past — otherwise the recovery
    // erases the evidence, which is the one thing an audit log must never do.
    log.reanchor()
        .expect("re-anchoring an examined log succeeds");
    log.append("scan `after`", "", 0)
        .expect("appends resume once the chain is anchored again");
    log.verify().expect("the re-anchored chain verifies");

    let records = log.records().expect("the log parses");
    let anchor = records
        .iter()
        .find(|record| record.query().contains("re-anchor"))
        .expect("the re-anchor is itself a record");
    // Link 2, not link 1: record 1 was removed, and the first link that does not *hold* is record 2,
    // whose predecessor is gone. That is the number AC-30 asks for and the number an operator acts on.
    assert!(
        anchor.query().contains("broken link 2"),
        "the anchor must name the first broken link: {:?}",
        anchor.query(),
    );
    assert!(
        records.len() >= 4,
        "nothing before the break may be discarded — the evidence stays: {records:#?}",
    );
}

#[test]
fn re_anchoring_a_sound_log_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let log = AuditLog::open(root.path());
    log.append("scan `t`", "", 0).expect("an append succeeds");

    let refused = log
        .reanchor()
        .expect_err("a log that verifies has nothing to re-anchor");
    assert!(
        matches!(&refused, euledb_storage::AuditError::NothingToReanchor),
        "re-anchoring a sound chain would obscure it for nothing: {refused:?}",
    );
}

/// A forged re-anchor must not launder a break.
///
/// The marker is what makes a mid-log anchor legitimate, so an attacker who can write the file could try
/// to append one by hand. They can — and it buys them nothing, because the records before it stay and
/// the anchor they wrote names the break itself.
#[test]
fn a_hand_written_anchor_cannot_erase_what_came_before() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let log = AuditLog::open(root.path());
    for round in 0..4 {
        log.append(&format!("scan `t{round}`"), "", round)
            .expect("an append succeeds");
    }

    let path = root.path().join(".euledb-audit.log");
    let before = std::fs::read_to_string(&path).expect("the log is readable");
    let kept: Vec<&str> = before
        .lines()
        .filter(|line| !line.starts_with("2\t"))
        .collect();
    std::fs::write(&path, kept.join("\n") + "\n").expect("the log is writable");

    // The break is visible, and re-anchoring does not remove the three records that remain.
    let broken = log.verify().expect_err("the chain is broken");
    assert!(
        matches!(&broken, euledb_storage::AuditError::BrokenChain { at } if *at == 3),
        "record 3 is the first whose predecessor is gone: {broken:?}",
    );
    log.reanchor().expect("re-anchoring succeeds");

    let records = log.records().expect("the log parses");
    assert_eq!(
        records
            .iter()
            .map(AuditRecord::sequence)
            .collect::<Vec<u64>>(),
        vec![0, 1, 3, 4],
        "the gap at 2 stays visible — an anchor moves forward, it does not tidy up behind itself",
    );
}

/// An anchor without the marker must not launder the records before it.
///
/// The marker is what makes a mid-log anchor legitimate. Without requiring it, anyone who can write the
/// file could silence a break by appending one bare record whose predecessor is the anchor value —
/// verification would start reading from there and everything earlier would go unexamined.
///
/// The forged record is internally consistent, hash included. That is not a reimplementation of the
/// production hash for its own sake: an attacker who can write the file can compute hashes too, and a
/// forgery with a wrong hash would be caught by the content check instead, which would make this test
/// pass for the wrong reason.
#[test]
fn an_anchor_without_the_marker_does_not_launder_the_break() {
    use sha2::{Digest as _, Sha256};

    let root = tempfile::tempdir().expect("a temporary directory is available");
    let log = AuditLog::open(root.path());
    for round in 0..4 {
        log.append(&format!("scan `t{round}`"), "", round)
            .expect("an append succeeds");
    }

    let path = root.path().join(".euledb-audit.log");
    let raw = std::fs::read_to_string(&path).expect("the log is readable");
    let kept: Vec<&str> = raw
        .lines()
        .filter(|line| !line.starts_with("2\t"))
        .collect();

    // A record that anchors a chain but does not say it is a re-anchor, sealed the way the format does.
    let (sequence, query, plan, rows) = (4_u64, "scan `harmless`", "", 0_u64);
    let mut digest = Sha256::new();
    digest.update([0_u8; 32]);
    digest.update(sequence.to_be_bytes());
    digest.update((query.len() as u64).to_be_bytes());
    digest.update(query.as_bytes());
    digest.update((plan.len() as u64).to_be_bytes());
    digest.update(plan.as_bytes());
    digest.update(rows.to_be_bytes());
    let hash: [u8; 32] = digest.finalize().into();
    let hex = hash.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
        out
    });
    let forged = format!(
        "{sequence}\t{}\t{hex}\t{rows}\t{query}\t{plan}",
        "0".repeat(64)
    );

    let mut lines = kept.join("\n");
    lines.push('\n');
    lines.push_str(&forged);
    lines.push('\n');
    std::fs::write(&path, lines).expect("the log is writable");

    let broken = log
        .verify()
        .expect_err("a bare anchor must not make a broken chain verify");
    assert!(
        matches!(&broken, euledb_storage::AuditError::BrokenChain { at } if *at == 3),
        "the break at record 3 must still be reported, not skipped over: {broken:?}",
    );
}
