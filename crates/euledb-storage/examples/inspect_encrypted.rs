//! Which objects on disk actually carry the framing, and how much the framing costs.
//!
//! ```text
//! cargo run --release --example inspect_encrypted -p euledb-storage
//! ```

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "an inspection tool reports"
)]

use std::sync::Arc;
use std::time::Instant;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{Compression, Keyring, LanceStore, TableDefinition, TableSchema, TableStore};

fn table() -> TableDefinition {
    TableDefinition::new(TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("body", DataType::Utf8, false),
    ])))
    .with_compression(Compression::none())
}

fn rows(count: i64) -> RecordBatch {
    let id: ArrayRef = Arc::new(Int64Array::from((0..count).collect::<Vec<i64>>()));
    let body: ArrayRef = Arc::new(StringArray::from(
        (0..count)
            .map(|row| format!("Grundsatzurteil zur Vorratsdatenspeicherung Rn. {row}"))
            .collect::<Vec<String>>(),
    ));
    RecordBatch::try_from_iter_with_nullable([("id", id, false), ("body", body, false)])
        .expect("batch")
}

fn walk(root: &std::path::Path) -> Vec<(String, u64, bool)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let bytes = std::fs::read(&path).unwrap_or_default();
            out.push((
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
                bytes.len() as u64,
                bytes.starts_with(b"EULE"),
            ));
        }
    }
    out.sort();
    out
}

async fn write_table(encrypted: bool, count: i64) -> (u64, std::time::Duration) {
    let root = tempfile::tempdir().expect("tmp");
    let keyring = Keyring::create("passphrase").expect("keyring");
    let store = if encrypted {
        LanceStore::new(root.path()).encrypted(&keyring)
    } else {
        LanceStore::new(root.path())
    };
    let started = Instant::now();
    store.create_table("t", &table()).await.expect("create");
    store.append("t", &rows(count)).await.expect("append");
    let read = store.scan("t").await.expect("scan");
    let elapsed = started.elapsed();
    assert_eq!(
        read.iter().map(RecordBatch::num_rows).sum::<usize>(),
        count as usize
    );
    let total: u64 = walk(root.path()).iter().map(|(_, size, _)| size).sum();
    (total, elapsed)
}

#[tokio::main]
async fn main() {
    println!("\n=== which objects are framed (20 000 rows, compression off) ===");
    let root = tempfile::tempdir().expect("tmp");
    let keyring = Keyring::create("passphrase").expect("keyring");
    let store = LanceStore::new(root.path()).encrypted(&keyring);
    store.create_table("t", &table()).await.expect("create");
    store.append("t", &rows(20_000)).await.expect("append");
    for (name, size, framed) in walk(root.path()) {
        println!("  {name:<58} {size:>9} bytes  framed={framed}");
    }

    println!("\n=== cost of encryption: size and the round trip (best of 5) ===");
    for count in [20_000_i64, 200_000, 1_000_000] {
        let mut plain = Vec::new();
        let mut enc = Vec::new();
        let mut plain_bytes = 0;
        let mut enc_bytes = 0;
        for _ in 0..5 {
            let (b, t) = write_table(false, count).await;
            plain_bytes = b;
            plain.push(t.as_secs_f64() * 1000.0);
            let (b, t) = write_table(true, count).await;
            enc_bytes = b;
            enc.push(t.as_secs_f64() * 1000.0);
        }
        plain.sort_by(f64::total_cmp);
        enc.sort_by(f64::total_cmp);
        let overhead = (enc_bytes as f64 / plain_bytes as f64 - 1.0) * 100.0;
        println!(
            "  {count:>9} rows  plain={plain_bytes:>11} enc={enc_bytes:>11} (+{overhead:>5.2} %)   \
             write+read plain={:>7.1} ms enc={:>7.1} ms ({:.2}x)",
            plain[0],
            enc[0],
            enc[0] / plain[0],
        );
    }
}
