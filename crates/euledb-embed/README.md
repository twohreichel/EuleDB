# euledb-embed

Embeddings for [EuleDB](https://github.com/twohreichel/EuleDB): the `multilingual-e5-small` ONNX graph, run
locally in pure Rust.

The model's own exported graph is executed rather than a Rust re-implementation of its architecture, so the
vectors are *the model's* and not an approximation nobody else can reproduce. 384 dimensions, chunked to the
model's 512-token window, E5 prefixes applied, mean-pooled and L2-normalised.

**The weights are not bundled.** Half a gigabyte does not belong in a crate, and the model carries its own
licence. Fetch it once at a pinned revision — the repository's `just model` does exactly this — and point
`Embedder::load` at the directory.

```rust
use euledb_embed::Embedder;

let embedder = Embedder::load("model")?;
let stored = embedder.embed_passage("Als Flut wird das Steigen des Wasserstandes bezeichnet.")?;
let query = embedder.embed_query("Wie hängen Ebbe und Flut zusammen?")?;
# Ok::<(), euledb_embed::EmbedError>(())
```

The suite runs this against the real model in `crates/euledb-embed/tests/embedding.rs` rather than against
a stand-in, so what is asserted about the vectors is the model's behaviour and not the wiring's.

Nothing leaves the machine: the graph runs on the CPU, and no network call happens after the model is on
disk.

- Licence: `Apache-2.0 OR MIT`
