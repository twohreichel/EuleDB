# euledb

Local-first embedded hybrid database. One encrypted file on disk that fuses three retrieval paths —
exact filters, vector semantics and BM25 full text — with no server, no daemon and no network call on
the query path.

**Early development, and the hybrid retrieval the first paragraph describes is not built yet.** What
works today is the storage foundation: declare a table, insert rows, scan, update, delete and drop it,
compressed, and encrypted at rest with AES-256-GCM under a rotatable key. One writer at a time, any
number of readers. There is no vector search, no full-text search and no plain-language query path —
those are later phases. The cryptographic design has not been audited.

```rust
use euledb::arrow_schema::{DataType, Field, Schema};
use euledb::{Database, Result, TableSchema};

async fn declare_a_table() -> Result<()> {
    let database = Database::open_for_writing("./library")?;
    let schema = TableSchema::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
    ]));
    database.create_table("documents", &schema).await
}
```

The same example, with rows going in and coming back out, is on `Database` in the API documentation,
where the test suite compiles and runs it.

Every tunable lives on `Config`, and `Database::encrypted` takes a `Keyring`.

What it is deliberately **not**: a server, a general-purpose SQL engine, a cloud sync service, or a
platform for training models. Analytical workloads belong in DuckDB.

- Repository, roadmap and specification: <https://github.com/twohreichel/EuleDB>
- Licence: `Apache-2.0 OR MIT`
