#![forbid(unsafe_code)]

//! Embeddings for EuleDB: the model's own ONNX graph, run locally.
//!
//! **Why the graph and not a Rust re-implementation of the architecture.** The vectors have to be *the
//! model's*, not an approximation that is merely self-consistent — a hand-written transformer that
//! differs in pooling or masking produces embeddings nobody else can reproduce, and then every published
//! recall number is about this code rather than about the model. Running the exported graph is what makes
//! the computation the reference one.
//!
//! **Why `tract`.** Three runtimes were weighed and both alternatives failed on something this project
//! cannot give up. `ort` downloads a prebuilt C++ runtime at build time over TLS, outside every gate this
//! project has. `candle-onnx` is pure Rust, and its `gemm` dependency emits aarch64 assembly requiring
//! the `fullfp16` CPU feature — so it does not build on linux-aarch64 with default target features, one
//! of the four platforms this project claims to support. `tract` has neither problem and needs no build
//! tool at all.
//!
//! The model itself is not tracked. `just model` fetches it at a pinned revision.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tract_onnx::prelude::*;

/// The model's hidden size, and therefore the length of every vector it produces.
pub const DIMENSIONS: usize = 384;

/// The model's context window, in tokens.
pub const TOKEN_LIMIT: usize = 512;

/// The lengths the graph is compiled for.
///
/// **Why buckets rather than one length or every length.** `tract` compiles a graph for a fixed shape —
/// the attention layers reshape in a way it cannot resolve symbolically. Padding everything to the full
/// window was measured at **108 ms per call**, which spends the entire per-query latency budget on one
/// embedding. Compiling per exact length instead costs about 130 ms each and would compile one plan per
/// distinct token count. Buckets bound the waste to under a factor of two and the number of plans to six:
/// a short query runs in about 4 ms, a full chunk in about 108 ms, and nothing in between is padded more
/// than twice its size.
const BUCKETS: [usize; 6] = [16, 32, 64, 128, 256, TOKEN_LIMIT];

/// The prefix E5 expects on stored text.
const PASSAGE_PREFIX: &str = "passage: ";

/// The prefix E5 expects on a query.
///
/// Not decoration: E5 is trained with these, and omitting them costs measurable recall.
const QUERY_PREFIX: &str = "query: ";

/// The token the model pads with, from its own configuration.
const PAD_TOKEN: i64 = 1;

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

/// A graph compiled for one bucket length.
type Plan = tract_onnx::prelude::TypedSimplePlan;

/// The embedding model, loaded once and used many times.
///
/// **When** — construct one per process and share it: loading half a gigabyte of weights per call would
/// dominate every measurement. **Where** — entirely local, which is the project's premise.
pub struct Embedder {
    graph: PathBuf,
    tokenizer: tokenizers::Tokenizer,
    /// One compiled plan per bucket, built on first use rather than all at once — a process that only
    /// ever embeds queries should not pay for the plan a full chunk needs.
    plans: Mutex<BTreeMap<usize, std::sync::Arc<Plan>>>,
}

impl std::fmt::Debug for Embedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The tokenizer holds a 250 000-entry vocabulary. Printing it would be useless to a reader and
        // ruinous to a log.
        f.debug_struct("Embedder")
            .field("graph", &self.graph)
            .finish_non_exhaustive()
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

        let tokenizer = tokenizers::Tokenizer::from_file(&vocabulary).map_err(|cause| {
            EmbedError::ModelUnreadable {
                path: vocabulary.clone(),
                cause: cause.to_string(),
            }
        })?;

        Ok(Self {
            graph,
            tokenizer,
            plans: Mutex::new(BTreeMap::new()),
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
    /// graph cannot be compiled or evaluated.
    pub fn embed_passage(&self, text: &str) -> Result<Vec<Embedding>, EmbedError> {
        self.chunks(text)?
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
        let budget = TOKEN_LIMIT.saturating_sub(self.token_count(PASSAGE_PREFIX)? + 2);

        let encoded = self.encode(text)?;
        if encoded.get_ids().len() <= budget {
            return Ok(vec![text.to_owned()]);
        }

        // Walk the token offsets so a chunk boundary lands between tokens rather than inside a character.
        let mut chunks = Vec::new();
        let offsets = encoded.get_offsets();
        let mut start = 0;
        let mut taken = 0;
        for (index, (_, end)) in offsets.iter().enumerate() {
            taken += 1;
            if taken >= budget || index + 1 == offsets.len() {
                if let Some(piece) = text.get(start..*end).filter(|s| !s.trim().is_empty()) {
                    chunks.push(piece.to_owned());
                }
                start = *end;
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

    /// The compiled plan for a bucket, building it on first use.
    fn plan(&self, length: usize) -> Result<std::sync::Arc<Plan>, EmbedError> {
        let mut plans = self.plans.lock().map_err(|_| EmbedError::Inference {
            cause: "the plan cache was poisoned by an earlier panic".to_owned(),
        })?;
        if let Some(plan) = plans.get(&length) {
            return Ok(std::sync::Arc::clone(plan));
        }

        let fact = i64::fact([1, length]);
        // `into_runnable` already hands back a shared plan, so nothing is wrapped a second time.
        let build = || -> TractResult<std::sync::Arc<Plan>> {
            tract_onnx::onnx()
                .model_for_path(&self.graph)?
                .with_input_fact(0, fact.clone().into())?
                .with_input_fact(1, fact.clone().into())?
                .with_input_fact(2, fact.into())?
                .into_optimized()?
                .into_runnable()
        };
        let plan = build().map_err(|cause| EmbedError::Inference {
            cause: cause.to_string(),
        })?;
        plans.insert(length, std::sync::Arc::clone(&plan));
        Ok(plan)
    }

    /// One forward pass, mean-pooled over the attention mask and L2-normalised.
    ///
    /// Mean pooling is what E5 is trained for: taking the leading token instead would produce vectors
    /// that are stable, cheap and wrong.
    fn run(&self, text: &str) -> Result<Embedding, EmbedError> {
        let encoded = self
            .tokenizer
            .encode(text, true)
            .map_err(|cause| EmbedError::Tokenizer {
                cause: cause.to_string(),
            })?;

        let real = encoded.get_ids().len();
        let bucket = BUCKETS
            .iter()
            .copied()
            .find(|candidate| *candidate >= real)
            .unwrap_or(TOKEN_LIMIT);

        // Pad to the bucket and mask the padding. Everything after `real` carries no information, and
        // pooling it in would pull every short text towards the same vector.
        let mut ids = vec![PAD_TOKEN; bucket];
        let mut mask = vec![0_i64; bucket];
        for (slot, id) in ids.iter_mut().zip(encoded.get_ids()) {
            *slot = i64::from(*id);
        }
        for slot in mask.iter_mut().take(real.min(bucket)) {
            *slot = 1;
        }

        let inference = |cause: TractError| EmbedError::Inference {
            cause: cause.to_string(),
        };
        let tensor = |values: Vec<i64>| -> Result<TValue, EmbedError> {
            Ok(tract_ndarray::Array2::from_shape_vec((1, bucket), values)
                .map_err(|cause| EmbedError::Inference {
                    cause: cause.to_string(),
                })?
                .into_tensor()
                .into())
        };

        let outputs = self
            .plan(bucket)?
            .run(tvec!(
                tensor(ids)?,
                tensor(mask.clone())?,
                tensor(vec![0_i64; bucket])?
            ))
            .map_err(inference)?;

        let hidden = outputs
            .into_iter()
            .next()
            .ok_or_else(|| EmbedError::Inference {
                cause: "the graph produced no output".to_owned(),
            })?
            .into_tensor();
        let view = hidden.to_plain_array_view::<f32>().map_err(inference)?;
        let values: Vec<f32> = view.iter().copied().collect();

        Ok(Embedding(normalise(mean_pool(&values, &mask))))
    }
}

/// Average the token vectors the attention mask keeps.
///
/// **Mean pooling, not the leading token.** E5 is trained with mean pooling, and taking the first token
/// instead produces vectors that are stable, cheap and *wrong* — self-consistent embeddings that are not
/// the model's, which is exactly the failure that running the exported graph exists to avoid.
///
/// The mask is load-bearing because inputs are padded to a bucket: a padded position carries no
/// information, and including it would pull every short text towards the same vector.
fn mean_pool(values: &[f32], mask: &[i64]) -> Vec<f32> {
    let mut summed = vec![0.0_f32; DIMENSIONS];
    let mut counted = 0.0_f32;
    // `as_chunks` rather than `chunks_exact`: clippy on a newer stable than the pinned toolchain insists,
    // and it is right — the length is a constant, so the array type carries it and the remainder is
    // explicit rather than discarded silently.
    let (rows, remainder) = values.as_chunks::<DIMENSIONS>();
    debug_assert!(
        remainder.is_empty(),
        "the graph's output is a whole number of {DIMENSIONS}-wide rows",
    );
    for (row, keep) in rows.iter().zip(mask) {
        if *keep == 0 {
            continue;
        }
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

    /// The graph could not be compiled or evaluated.
    #[error("the model could not be evaluated: {cause}")]
    Inference {
        /// What the runtime said.
        cause: String,
    },
}
