//! On-disk sizes for every encoding option, so the defaults are chosen from numbers rather than from
//! the documentation's adjectives.
//!
//! Run it with:
//!
//! ```text
//! cargo run --release --example measure_encoding -p euledb-storage
//! ```
//!
//! Deliberately an example rather than a test. It asserts nothing — it reports — and a test that
//! asserts nothing is noise in a suite. The relationships it revealed ARE asserted, in
//! `tests/compression.rs`, at a corpus size small enough to keep the suite fast.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a measurement harness reports rather than recovers, and a panic here is the report"
)]

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, RecordBatchIterator, StringArray};
use arrow_schema::{DataType, Field, Schema};

/// A realistic multilingual corpus: repetitive legal-style prose, which is what a document table holds
/// and what string compression is supposed to exploit.
fn corpus(rows: usize) -> (Vec<i64>, Vec<String>, Vec<String>) {
    let templates = [
        "Der Erste Senat des Bundesverfassungsgerichts hat entschieden, dass die Vorschrift mit dem Grundgesetz unvereinbar ist",
        "Le Conseil constitutionnel a jugé que la disposition contestée méconnaît le droit au respect de la vie privée",
        "Trybunał orzekł, że przepis narusza zasadę proporcjonalności wynikającą z Konstytucji Rzeczypospolitej",
        "De Hoge Raad heeft geoordeeld dat de bepaling in strijd is met het recht op eerbiediging van de persoonlijke levenssfeer",
    ];
    let languages = ["de", "fr", "pl", "nl"];
    let mut ids = Vec::with_capacity(rows);
    let mut bodies = Vec::with_capacity(rows);
    let mut langs = Vec::with_capacity(rows);
    for row in 0..rows {
        ids.push(4218 + row as i64);
        let pick = row % templates.len();
        bodies.push(format!("{} (Rn. {})", templates[pick], row));
        langs.push(languages[pick].to_owned());
    }
    (ids, bodies, langs)
}

fn schema_with(metadata: &[(&str, HashMap<String, String>)]) -> Schema {
    let fields = ["id", "body", "language"].map(|name| {
        let data_type = if name == "id" {
            DataType::Int64
        } else {
            DataType::Utf8
        };
        let mut field = Field::new(name, data_type, false);
        if let Some((_, meta)) = metadata.iter().find(|(field_name, _)| *field_name == name) {
            field = field.with_metadata(meta.clone());
        }
        field
    });
    Schema::new(fields.to_vec())
}

fn meta(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// Sum only the data files. The manifest and version files vary between runs, and including them made
/// the default look noisy when the data itself was not.
fn data_bytes(root: &std::path::Path) -> u64 {
    fn walk(path: &std::path::Path, out: &mut u64) {
        for entry in std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "lance") {
                *out += p.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    let mut total = 0;
    walk(root, &mut total);
    total
}

async fn write_once(schema: Schema, rows: usize) -> u64 {
    let dir = tempfile::tempdir().expect("temp dir");
    let uri = dir.path().join("t.lance").display().to_string();
    let (ids, bodies, langs) = corpus(rows);
    let schema = Arc::new(schema);
    let id: ArrayRef = Arc::new(Int64Array::from(ids));
    let body: ArrayRef = Arc::new(StringArray::from(bodies));
    let language: ArrayRef = Arc::new(StringArray::from(langs));
    let batch = RecordBatch::try_new(schema.clone(), vec![id, body, language]).expect("batch");
    let reader = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema);
    lance::Dataset::write(reader, uri.as_str(), None)
        .await
        .expect("write");

    data_bytes(dir.path())
}

/// Three runs, reported as a range, so a claim about a difference can be told apart from noise.
async fn write_and_measure(label: &str, schema: Schema, rows: usize) -> u64 {
    let mut sizes = Vec::new();
    for _ in 0..3 {
        sizes.push(write_once(schema.clone(), rows).await);
    }
    sizes.sort_unstable();
    let (min, max) = (sizes[0], sizes[2]);
    let spread = if min == max {
        "stable".to_owned()
    } else {
        format!("+{}", max - min)
    };
    println!("  {label:<44} {min:>9} bytes  ({spread})");
    min
}

#[tokio::main]
async fn main() {
    // Larger than the suite uses, because the point here is the shape of the numbers rather than a
    // fast gate. The differences are visible from about 2 000 rows and stable well before this.
    let rows = 20_000;
    let (_, bodies, _) = corpus(rows);
    let raw: usize = bodies.iter().map(String::len).sum();
    println!("\n=== {rows} rows, {raw} bytes of raw body text ===");

    write_and_measure("default (no field metadata)", schema_with(&[]), rows).await;
    write_and_measure(
        "compression=none on both string columns",
        schema_with(&[
            ("body", meta(&[("lance-encoding:compression", "none")])),
            ("language", meta(&[("lance-encoding:compression", "none")])),
        ]),
        rows,
    )
    .await;
    for level in ["1", "3", "9", "19"] {
        write_and_measure(
            &format!("zstd level {level} on every column"),
            schema_with(&[
                (
                    "id",
                    meta(&[
                        ("lance-encoding:compression", "zstd"),
                        ("lance-encoding:compression-level", level),
                    ]),
                ),
                (
                    "body",
                    meta(&[
                        ("lance-encoding:compression", "zstd"),
                        ("lance-encoding:compression-level", level),
                    ]),
                ),
                (
                    "language",
                    meta(&[
                        ("lance-encoding:compression", "zstd"),
                        ("lance-encoding:compression-level", level),
                    ]),
                ),
            ]),
            rows,
        )
        .await;
    }
    write_and_measure(
        "fsst forced on both string columns",
        schema_with(&[
            ("body", meta(&[("lance-encoding:compression", "fsst")])),
            ("language", meta(&[("lance-encoding:compression", "fsst")])),
        ]),
        rows,
    )
    .await;
    for level in ["1", "9", "19"] {
        write_and_measure(
            &format!("zstd {level} on STRING columns only"),
            schema_with(&[
                (
                    "body",
                    meta(&[
                        ("lance-encoding:compression", "zstd"),
                        ("lance-encoding:compression-level", level),
                    ]),
                ),
                (
                    "language",
                    meta(&[
                        ("lance-encoding:compression", "zstd"),
                        ("lance-encoding:compression-level", level),
                    ]),
                ),
            ]),
            rows,
        )
        .await;
    }
    // Repeat the default to see whether the numbers move between runs at all.
    write_and_measure("default again (noise check)", schema_with(&[]), rows).await;
    write_and_measure(
        "zstd 3 on id only, strings left to lance",
        schema_with(&[(
            "id",
            meta(&[
                ("lance-encoding:compression", "zstd"),
                ("lance-encoding:compression-level", "3"),
            ]),
        )]),
        rows,
    )
    .await;
}
