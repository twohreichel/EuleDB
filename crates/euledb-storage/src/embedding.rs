//! The seam between a text column and whatever turns text into vectors.

use crate::measurement::RowId;

/// How wide a vector is.
///
/// Fixed here rather than read from the embedder, because it is part of the companion table's schema and
/// a table cannot change its shape because a different model was loaded. An embedder producing another
/// width is refused rather than silently stored.
pub const VECTOR_WIDTH: usize = 384;

/// Where vectors come from.
///
/// **Why a port** — the model is 470 MB of weights and about 200 crates of dependency tree, and the
/// storage layer needs neither. The dependency points inwards: this layer decides *when* text is embedded
/// and the adapter decides *how*.
pub trait Embedder: std::fmt::Debug + Send + Sync {
    /// Embed stored text, one vector per chunk.
    ///
    /// # Errors
    ///
    /// Whatever the implementation's own failure is, as a message. The storage layer passes it through
    /// rather than interpreting it.
    fn embed_passage(&self, text: &str) -> Result<Vec<Vec<f32>>, String>;
}

/// One vector, and the row it was made from.
///
/// **Why a chunk index** — a document longer than the model's window is several vectors, so a row owns an
/// ordered set of them rather than one. A hit is therefore a *chunk*, which resolves to a row: that is
/// what makes a long document findable by any of its parts instead of only its beginning.
#[derive(Debug, Clone, PartialEq)]
pub struct RowVector {
    /// The row this came from.
    pub row: RowId,
    /// Which chunk of that row's text, counting from zero.
    pub chunk: u32,
    /// The vector itself, L2-normalised by the embedder.
    pub embedding: Vec<f32>,
}
