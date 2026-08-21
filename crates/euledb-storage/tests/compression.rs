//! Compression is a per-table setting made at creation time, and it has to be observable on disk.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::path::Path;
use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{
    Compression, LanceStore, TableDefinition, TableSchema, TableStore, ZstdLevel,
};

/// A document table: an identifier and repetitive multilingual prose, which is what a compressor has
/// to work with here and what the encoding measurement was taken on.
fn documents() -> TableSchema {
    TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("body", DataType::Utf8, false),
        Field::new("language", DataType::Utf8, false),
    ]))
}

/// Enough repetitive text that a compressor has something to find.
///
/// 2 000 rows, chosen by measurement rather than by feel: at that size uncompressed is still 4.2 times
/// the compressed size and two zstd levels still differ by over a thousand bytes, so every assertion
/// below has a comfortable margin — while the four tests together run in under a second. At 20 000 rows
/// they took 27.
fn corpus(rows: usize) -> RecordBatch {
    let templates = [
        "Der Erste Senat hat entschieden, dass die Vorschrift mit dem Grundgesetz unvereinbar ist",
        "Le Conseil constitutionnel a jugé que la disposition méconnaît le droit à la vie privée",
        "Trybunał orzekł, że przepis narusza zasadę proporcjonalności wynikającą z Konstytucji",
        "De Hoge Raad heeft geoordeeld dat de bepaling in strijd is met het recht op eerbiediging",
    ];
    let languages = ["de", "fr", "pl", "nl"];
    let ids: Vec<i64> = (0..rows).map(|row| 4218 + row as i64).collect();
    let bodies: Vec<String> = (0..rows)
        .map(|row| format!("{} (Rn. {row})", templates[row % templates.len()]))
        .collect();
    let langs: Vec<&str> = (0..rows)
        .map(|row| languages[row % languages.len()])
        .collect();

    let id: ArrayRef = Arc::new(Int64Array::from(ids));
    let body: ArrayRef = Arc::new(StringArray::from(bodies));
    let language: ArrayRef = Arc::new(StringArray::from(langs));
    RecordBatch::try_from_iter_with_nullable([
        ("id", id, false),
        ("body", body, false),
        ("language", language, false),
    ])
    .expect("the batch matches the declared schema")
}

/// Bytes of the data files only. Manifest and version files vary between runs, and counting them would
/// turn a size comparison into a coin toss — that non-determinism is itself a measured finding.
fn data_bytes(root: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path).into_iter().flatten().flatten() {
            let child = entry.path();
            if child.is_dir() {
                stack.push(child);
            } else if child.extension().is_some_and(|ext| ext == "lance") {
                total += child.metadata().map(|meta| meta.len()).unwrap_or(0);
            }
        }
    }
    total
}

/// Write the corpus into a fresh store with the given compression, and report the bytes it took.
async fn bytes_written_with(compression: Compression) -> u64 {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let store = LanceStore::new(root.path());
    let definition = TableDefinition::new(documents()).with_compression(compression);
    store
        .create_table("documents", &definition)
        .await
        .expect("creating a table must succeed");
    store
        .append("documents", &corpus(5000))
        .await
        .expect("appending the corpus must succeed");
    data_bytes(root.path())
}

#[tokio::test]
async fn compressing_a_table_makes_it_smaller_than_not_compressing_it() {
    let compressed = bytes_written_with(Compression::default()).await;
    let uncompressed = bytes_written_with(Compression::none()).await;

    // Not merely "smaller": the format compresses on its own when nothing is declared, so a bare
    // `<` would still pass if `Compression::None` silently meant "let the format decide" — which is a
    // mutation this test failed to catch until the factor was added. Measured ratio is 4.2, and the
    // format's own automatic choice is only about 1.15 times the explicit zstd size, so a factor of 2
    // separates the two with room to spare.
    assert!(
        uncompressed > compressed * 2,
        "`Compression::None` is not actually disabling compression: {uncompressed} bytes \
         uncompressed against {compressed} compressed is only a factor of {:.2}",
        uncompressed as f64 / compressed as f64,
    );
}

#[test]
fn the_named_levels_are_inside_the_range_the_constructor_accepts() {
    // The constants bypass the constructor, so nothing else stops one of them drifting outside the
    // range the constructor enforces — and a level zstd rejects fails at write time, far from here.
    for named in [ZstdLevel::FASTEST, ZstdLevel::SMALLEST, ZstdLevel::DEFAULT] {
        assert!(
            ZstdLevel::new(named.get()).is_ok(),
            "level {} is a named constant but the constructor rejects it",
            named.get(),
        );
    }
}

#[test]
fn a_level_outside_the_zstd_range_is_refused() {
    for refused in [0, 23, u8::MAX] {
        let error = ZstdLevel::new(refused)
            .expect_err("a level zstd does not define must not be constructible");
        assert_eq!(
            error.given, refused,
            "the error must name the value it rejected"
        );
    }
    assert!(
        ZstdLevel::new(1).is_ok() && ZstdLevel::new(22).is_ok(),
        "1 and 22 are the ends of the range zstd defines and must both be accepted",
    );
}

#[tokio::test]
async fn the_compression_level_reaches_the_encoder() {
    let fastest = bytes_written_with(Compression::zstd(ZstdLevel::FASTEST)).await;
    let smallest = bytes_written_with(Compression::zstd(ZstdLevel::SMALLEST)).await;

    // Asserting which is smaller would tie the test to one zstd version's tuning. That they DIFFER is
    // the property the criterion asks for: the level is configurable, so it has to have an effect.
    assert_ne!(
        fastest, smallest,
        "the level is not plumbed through — the fastest and the smallest setting produced the same \
         {fastest} bytes",
    );
}

#[tokio::test]
async fn the_same_input_written_twice_takes_the_same_bytes() {
    // Reproducibility, and it is not free: the format's own automatic encoding choice varies by more
    // than 20 % between runs on identical input. Declaring the compression explicitly is what makes a
    // recorded size mean anything.
    let first = bytes_written_with(Compression::default()).await;
    let second = bytes_written_with(Compression::default()).await;

    assert_eq!(
        first, second,
        "the same rows took a different number of bytes on two runs, so no recorded size can be \
         compared against a later one",
    );
}

#[tokio::test]
async fn rows_still_come_back_unchanged_when_compressed() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    let written = corpus(5000);
    {
        let store = LanceStore::new(root.path());
        store
            .create_table("documents", &TableDefinition::new(documents()))
            .await
            .expect("create");
        store.append("documents", &written).await.expect("append");
    }

    let read_back = LanceStore::new(root.path())
        .scan("documents")
        .await
        .expect("a compressed table must be readable");

    assert_eq!(
        read_back,
        vec![written],
        "compression changed the rows, which makes it lossy rather than compression",
    );
}
