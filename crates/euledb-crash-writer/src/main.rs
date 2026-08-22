#![forbid(unsafe_code)]

//! Appends batches to a database until something kills it.
//!
//! A test fixture, not a tool. The crash-safety criterion asks for proof by killing the writer at
//! several points rather than by argument, and that needs a process which is genuinely writing when the
//! signal arrives. Simulating the failure inside the test would only show that the simulation agrees
//! with itself.
//!
//! ```text
//! euledb-crash-writer <directory> <rows-per-batch>
//! ```
//!
//! It never exits on its own. Whoever started it is expected to kill it.

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use euledb_storage::{LanceStore, TableDefinition, TableSchema, TableStore};

/// Exit code for a usage error, distinct from being killed.
const USAGE: i32 = 2;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(root), Some(batch_size)) = (args.next(), args.next()) else {
        eprintln!("usage: euledb-crash-writer <directory> <rows-per-batch>");
        std::process::exit(USAGE);
    };
    let Ok(batch_size) = batch_size.parse::<i64>() else {
        eprintln!("rows-per-batch must be a number");
        std::process::exit(USAGE);
    };

    let schema = TableSchema::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));
    let store = match LanceStore::open_for_writing(&root) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("cannot open {root} for writing: {err}");
            std::process::exit(USAGE);
        }
    };

    // Created only if it is not there. A second run against the same database — which is exactly what
    // happens after the first one was killed — must continue the table rather than demand a fresh one.
    // `create_table` refuses an existing table, and rightly so, but it does not yet say *why* in a way
    // this could match on: a distinct already-exists error belongs with the public error type.
    if store.scan("counters").await.is_err()
        && let Err(err) = store
            .create_table("counters", &TableDefinition::new(schema))
            .await
    {
        eprintln!("cannot create the table: {err}");
        std::process::exit(USAGE);
    }
    // Announced on stdout so the test knows the table exists before it starts timing.
    println!("ready");

    let mut written = 0_i64;
    loop {
        let id: ArrayRef = Arc::new(Int64Array::from(
            (written..written + batch_size).collect::<Vec<i64>>(),
        ));
        let Ok(batch) = RecordBatch::try_from_iter_with_nullable([("id", id, false)]) else {
            eprintln!("cannot build a batch");
            std::process::exit(USAGE);
        };
        if let Err(err) = store.append("counters", &batch).await {
            eprintln!("append failed: {err}");
            std::process::exit(USAGE);
        }
        written += batch_size;
    }
}
