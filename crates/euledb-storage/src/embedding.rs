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

    /// Embed a query.
    ///
    /// **A separate method, not the same one.** The model these vectors come from is trained with a
    /// different prefix for a query than for stored text, and using the wrong one costs measurable
    /// recall. Making the two indistinguishable at the port would make that mistake invisible.
    ///
    /// # Errors
    ///
    /// As [`Embedder::embed_passage`].
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String>;
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

/// Which vector index a column carries.
///
/// **Why a choice at all** — the two trade the same thing in opposite directions. The graph keeps every
/// vector and walks between them, which is fast and holds the collection in memory. Product quantisation
/// replaces each vector with a short code, which fits where memory is scarce and answers approximately
/// from a lossy representation.
///
/// The query API does not change with the choice: a caller asks for the nearest vectors and does not say
/// how they are found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VectorIndexKind {
    /// A navigable graph over the vectors themselves. The default, for small and mid-size collections.
    #[default]
    Graph,
    /// Product quantisation: each vector becomes a short code. For where memory is the constraint.
    Quantised,
}

/// The language a text index stems and removes stop words for.
///
/// **One language per index, and that is the method rather than a limitation of this code.** A Snowball
/// stemmer is language-specific: it strips German endings from German words, and applying it to French
/// would produce nonsense. A table holding several languages therefore wants an index per language, not
/// one index that tries to be all of them.
///
/// The list is what the stemmer library supports. **Polish is absent** — Snowball has no Polish stemmer,
/// so a Polish column is indexed without stemming whichever engine is used, and that is worth knowing
/// rather than discovering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StemmingLanguage {
    /// No stemming: tokens are indexed as they appear.
    None,
    /// German.
    German,
    /// English.
    English,
    /// French.
    French,
    /// Spanish.
    Spanish,
    /// Italian.
    Italian,
    /// Dutch.
    Dutch,
    /// Portuguese.
    Portuguese,
}

impl StemmingLanguage {
    /// The name the index configuration expects, and whether to stem at all.
    pub(crate) const fn as_parts(self) -> (&'static str, bool) {
        match self {
            Self::None => ("English", false),
            Self::German => ("German", true),
            Self::English => ("English", true),
            Self::French => ("French", true),
            Self::Spanish => ("Spanish", true),
            Self::Italian => ("Italian", true),
            Self::Dutch => ("Dutch", true),
            Self::Portuguese => ("Portuguese", true),
        }
    }
}
