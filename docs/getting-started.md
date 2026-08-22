# Getting started

EuleDB is a local database that answers three kinds of question about the same data — exact filters,
meaning, and words — and can fuse the last two into one ranking. Everything runs on your machine.

**Every example below is compiled and executed by the test suite**, in `crates/euledb/tests/api.rs` —
`seeded_library` for the setup and `a_newcomer_can_run_every_query_kind` for the queries. If the guide and
the API ever disagree, the suite fails rather than the guide quietly rotting.

## What you need

```sh
git clone https://github.com/twohreichel/EuleDB && cd EuleDB
just model    # fetches the embedding model once, ~490 MB, at a pinned revision
```

The model is needed only for meaning — semantic and hybrid queries, and inserting into a column that
embeds itself. Exact filters and full text need nothing extra.

You also need `protoc` on your `PATH`; `CONTRIBUTING.md` says why.

## Open a database

```rust
use std::sync::Arc;
use euledb::{Database, TableSchema};
use euledb::arrow_schema::{DataType, Field, Schema};

let embedder = Arc::new(euledb_embed::Embedder::load("model")?);
let db = Database::open_for_writing("./library")?.embedding(embedder);
```

`open_for_writing` takes the write role and holds it until the handle is dropped: many readers may hold the
same database at once, at most one writer. `Database::open` gives a reader.

## Declare a table

```rust
let schema = TableSchema::new(Schema::new(vec![
    Field::new("id", DataType::Int64, false),
    Field::new("title", DataType::Utf8, false),
    Field::new("body", DataType::Utf8, false),
]))
.auto_embedding("body");

db.create_table("documents", &schema).await?;
```

`auto_embedding` says that `body` embeds itself on every insert and update. You never refresh a vector by
hand — a vector that has to be refreshed by hand is a vector that will not be.

## Insert

```rust
db.insert("documents", &batch).await?;
```

The batch is an Arrow `RecordBatch` whose columns match the declaration by name. `euledb::arrow_array` and
`euledb::arrow_schema` are re-exported so you do not have to match a version by hand.

## Build the indexes

```rust
db.index_text("documents", "body", euledb::StemmingLanguage::German).await?;
db.index_vectors("documents", "body", euledb::VectorIndexKind::Graph).await?;
```

Both are operations rather than declarations: they are built over rows that already exist. Re-run either
after a large insert to cover the new rows.

One language per text index — a stemmer is language-specific, so a table holding several languages wants an
index per language.

## Ask the three questions

**An exact filter** reads rows without any index:

```rust
let rows = db.scan("documents").await?;
```

**Full text** ranks by BM25, and the stemmer does the work: `Wasserstand` finds a sentence that says
`Wasserstandes`.

```rust
let lexical = db.text_search("documents", "body", "Wasserstand", 5).await?;
```

The last argument bounds the result, so you can page through matches rather than receiving all of them.

**Meaning** finds rows that answer a question none of them words the same way:

```rust
let semantic = db
    .semantic_search("documents", "body", "Wie hängen Ebbe und Flut zusammen?", 2)
    .await?;
```

The query is embedded for you, with the prefix the model expects of a *query* rather than of stored text —
a distinction that costs recall when it is got wrong.

## Fuse the last two

```rust
let fused = db.hybrid_search("documents", "body", "Wasserstand bei Flut", 3).await?;

for hit in &fused.hits {
    println!(
        "row {:?}: score {:.4}, vector rank {:?}, lexical rank {:?}",
        hit.row, hit.score, hit.vector_rank, hit.lexical_rank
    );
}
println!("fused with k = {}", fused.effective_k);
```

Every hit carries the rank each side gave it, so you can see whether it came from meaning, from words, or
from both. A row both sides placed moderately well outranks one a single side placed first — that is what
the fusion is for.

`effective_k` is reported because it is not constant: a small corpus uses a smaller value, since with the
default the scores of rank 1 and rank 20 differ by a few percent.

## What is not here yet

Being plain about it is more useful than a roadmap. There is no plain-language query layer, no
multi-device sync and no Python binding. The cryptographic design has not been audited. What works today is
the storage layer, the three retrieval paths and their fusion.
