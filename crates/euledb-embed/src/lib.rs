#![forbid(unsafe_code)]

//! Embeddings for EuleDB: the model's own ONNX graph, run locally.
//!
//! **Why the graph and not a Rust re-implementation of the architecture.** The vectors have to be *the
//! model's*, not an approximation that is merely self-consistent — a hand-written transformer that
//! differs in pooling or masking produces embeddings nobody else can reproduce, and then every published
//! recall number is about this code rather than about the model. Running the exported graph is what makes
//! the computation the reference one.
//!
//! **Why `candle-onnx` and not `ort`.** `ort`'s default features download a prebuilt C++ runtime at build
//! time over TLS, which sits outside every gate this project has: `cargo-deny` sees Rust crates, and CI
//! pins third-party actions to commit SHAs precisely to avoid unverified mutable artefacts. `candle-onnx`
//! is pure Rust and needs only `protoc`, which the on-disk format already requires. The cost is recorded:
//! a larger dependency tree, and a raised minimum toolchain.
//!
//! The model itself is not tracked. `just model` fetches it at a pinned revision.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use candle_core::{DType, Device, Tensor};

/// The model's hidden size, and therefore the length of every vector it produces.
pub const DIMENSIONS: usize = 384;

/// The model's context window, in tokens.
///
/// Text longer than this is chunked. The limit belongs to the model, not to a choice made here.
pub const TOKEN_LIMIT: usize = 512;

/// The prefix E5 expects on stored text.
const PASSAGE_PREFIX: &str = "passage: ";

/// The prefix E5 expects on a query.
///
/// Not decoration: E5 is trained with these, and omitting them costs measurable recall. Two texts that
/// differ only in prefix embed differently on purpose.
const QUERY_PREFIX: &str = "query: ";

/// One vector, L2-normalised.
///
/// Normalised on construction so cosine similarity is a dot product, and so nothing downstream has to
/// remember whether it was done.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding(Vec<f32>);

impl Embedding {
    /// The components.
    #[must_use]
    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }
}

/// The embedding model, loaded once and used many times.
///
/// **When** — construct one per process and share it: loading half a gigabyte of weights per call would
/// dominate every measurement. **Where** — entirely local, which is the project's premise.
pub struct Embedder {
    model: candle_onnx::onnx::ModelProto,
    tokenizer: tokenizers::Tokenizer,
    device: Device,
}

impl std::fmt::Debug for Embedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The weights are half a gigabyte and the tokenizer holds a 250 000-entry vocabulary. Printing
        // either would be useless to a reader and ruinous to a log.
        f.debug_struct("Embedder").finish_non_exhaustive()
    }
}

impl Embedder {
    /// Load the graph and its tokenizer from a directory `just model` filled.
    ///
    /// # Errors
    ///
    /// [`EmbedError::ModelMissing`] when a file is absent, naming the command that fetches it, and
    /// [`EmbedError::ModelUnreadable`] when one is there but is not what it should be.
    pub fn load(directory: impl AsRef<Path>) -> Result<Self, EmbedError> {
        let directory = directory.as_ref();
        let graph = directory.join("model.onnx");
        let vocabulary = directory.join("tokenizer.json");
        for file in [&graph, &vocabulary] {
            if !file.exists() {
                return Err(EmbedError::ModelMissing { path: file.clone() });
            }
        }

        let model =
            candle_onnx::read_file(&graph).map_err(|cause| EmbedError::ModelUnreadable {
                path: graph.clone(),
                cause: cause.to_string(),
            })?;
        let tokenizer = tokenizers::Tokenizer::from_file(&vocabulary).map_err(|cause| {
            EmbedError::ModelUnreadable {
                path: vocabulary.clone(),
                cause: cause.to_string(),
            }
        })?;

        Ok(Self {
            model,
            tokenizer,
            device: Device::Cpu,
        })
    }

    /// Embed stored text, chunked to the model's window.
    ///
    /// One vector per chunk, in order. A document longer than the window is several vectors rather than a
    /// truncated one, because truncating loses the end of every long document silently.
    ///
    /// # Errors
    ///
    /// [`EmbedError::Tokenizer`] when the text cannot be tokenised, [`EmbedError::Inference`] when the
    /// graph cannot be evaluated.
    pub fn embed_passage(&self, text: &str) -> Result<Vec<Embedding>, EmbedError> {
        let chunks = self.chunks(text)?;
        chunks
            .iter()
            .map(|chunk| self.run(&format!("{PASSAGE_PREFIX}{chunk}")))
            .collect()
    }

    /// Embed a query, which is short by nature and is never chunked.
    ///
    /// # Errors
    ///
    /// As [`Embedder::embed_passage`].
    pub fn embed_query(&self, text: &str) -> Result<Embedding, EmbedError> {
        self.run(&format!("{QUERY_PREFIX}{text}"))
    }

    /// Split text into pieces that fit the model's window once a prefix is added.
    ///
    /// Split on token counts rather than characters, because a character budget is wrong by a factor of
    /// three or more between Latin and non-Latin text — and this model is multilingual.
    ///
    /// Public because it is testable without a forward pass: chunking is tokenisation, and asserting it
    /// through [`Embedder::embed_passage`] would mean running the graph once per chunk to check an
    /// arithmetic property.
    ///
    /// # Errors
    ///
    /// [`EmbedError::Tokenizer`] when the text cannot be tokenised.
    pub fn chunks(&self, text: &str) -> Result<Vec<String>, EmbedError> {
        // The prefix costs tokens too, and the window is the model's hard limit rather than a target.
        let prefix_cost = self.token_count(PASSAGE_PREFIX)?;
        let budget = TOKEN_LIMIT.saturating_sub(prefix_cost + 2);

        let encoded = self.encode(text)?;
        if encoded.get_ids().len() <= budget {
            return Ok(vec![text.to_owned()]);
        }

        // Walk the token offsets so a chunk boundary lands between tokens rather than inside a character.
        let mut chunks = Vec::new();
        let offsets = encoded.get_offsets();
        let mut start_char = 0;
        let mut taken = 0;
        for (index, (_, end)) in offsets.iter().enumerate() {
            taken += 1;
            let last = index + 1 == offsets.len();
            if taken >= budget || last {
                let piece = text.get(start_char..*end).unwrap_or_default();
                if !piece.trim().is_empty() {
                    chunks.push(piece.to_owned());
                }
                start_char = *end;
                taken = 0;
            }
        }
        Ok(chunks)
    }

    /// How many tokens a string costs, without the special tokens the graph adds.
    ///
    /// Public for the same reason as [`Embedder::chunks`]: asserting a token budget should not cost a
    /// forward pass.
    ///
    /// # Errors
    ///
    /// [`EmbedError::Tokenizer`] when the text cannot be tokenised.
    pub fn token_count(&self, text: &str) -> Result<usize, EmbedError> {
        Ok(self.encode(text)?.get_ids().len())
    }

    /// Tokenise, without the special tokens the graph adds for itself.
    fn encode(&self, text: &str) -> Result<tokenizers::Encoding, EmbedError> {
        self.tokenizer
            .encode(text, false)
            .map_err(|cause| EmbedError::Tokenizer {
                cause: cause.to_string(),
            })
    }

    /// One forward pass, mean-pooled over the attention mask and L2-normalised.
    ///
    /// Mean pooling is what E5 is trained for: taking the first token instead would produce vectors that
    /// are stable, cheap and wrong.
    fn run(&self, text: &str) -> Result<Embedding, EmbedError> {
        let encoded = self
            .tokenizer
            .encode(text, true)
            .map_err(|cause| EmbedError::Tokenizer {
                cause: cause.to_string(),
            })?;

        let ids: Vec<i64> = encoded.get_ids().iter().map(|&id| i64::from(id)).collect();
        let mask: Vec<i64> = encoded
            .get_attention_mask()
            .iter()
            .map(|&flag| i64::from(flag))
            .collect();
        let length = ids.len();

        let inference = |cause: candle_core::Error| EmbedError::Inference {
            cause: cause.to_string(),
        };
        let mut inputs = HashMap::new();
        inputs.insert(
            "input_ids".to_owned(),
            Tensor::from_vec(ids, (1, length), &self.device).map_err(inference)?,
        );
        inputs.insert(
            "attention_mask".to_owned(),
            Tensor::from_vec(mask, (1, length), &self.device).map_err(inference)?,
        );
        inputs.insert(
            "token_type_ids".to_owned(),
            Tensor::zeros((1, length), DType::I64, &self.device).map_err(inference)?,
        );

        let outputs = candle_onnx::simple_eval(&self.model, inputs).map_err(inference)?;
        let hidden = outputs
            .get("last_hidden_state")
            .ok_or_else(|| EmbedError::Inference {
                cause: "the graph produced no last_hidden_state".to_owned(),
            })?;

        let values = hidden
            .flatten_all()
            .and_then(|flat| flat.to_vec1::<f32>())
            .map_err(inference)?;

        Ok(Embedding(normalise(mean_pool(&values))))
    }
}

/// Average every token vector the model produced.
///
/// **Mean pooling, not the first token.** E5 is trained with mean pooling, and taking the leading token
/// instead produces vectors that are stable, cheap and *wrong* — self-consistent embeddings that are not
/// the model's, which is exactly the failure that running the exported graph exists to avoid.
///
/// No attention mask is consulted, because there is nothing to mask: one text is encoded at a time and
/// nothing is padded. Masked pooling was written first and removed — a branch no caller can reach is a
/// branch no test can defend. It comes back with batching, together with a test that pads.
fn mean_pool(values: &[f32]) -> Vec<f32> {
    let mut summed = vec![0.0_f32; DIMENSIONS];
    let mut counted = 0.0_f32;
    for row in values.chunks_exact(DIMENSIONS) {
        for (slot, value) in summed.iter_mut().zip(row) {
            *slot += value;
        }
        counted += 1.0;
    }
    if counted > 0.0 {
        for slot in &mut summed {
            *slot /= counted;
        }
    }
    summed
}

/// Scale a vector to length one, so cosine similarity is a dot product.
fn normalise(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

/// The embedding path could not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EmbedError {
    /// A model file is not there.
    #[error("the model file {} is missing: run `just model` to fetch it", path.display())]
    ModelMissing {
        /// The file that was looked for.
        path: PathBuf,
    },

    /// A model file is there and is not usable.
    #[error("the model file {} could not be read: {cause}", path.display())]
    ModelUnreadable {
        /// The file that could not be used.
        path: PathBuf,
        /// What the loader said.
        cause: String,
    },

    /// Text could not be tokenised.
    #[error("the text could not be tokenised: {cause}")]
    Tokenizer {
        /// What the tokenizer said.
        cause: String,
    },

    /// The graph could not be evaluated.
    #[error("the model could not be evaluated: {cause}")]
    Inference {
        /// What the runtime said.
        cause: String,
    },
}
