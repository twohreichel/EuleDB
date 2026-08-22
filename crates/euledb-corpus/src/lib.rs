#![forbid(unsafe_code)]

//! The reference corpus this project's benchmarks are measured against.
//!
//! **Why a crate rather than a test fixture** — the same documents have to reach a test and a benchmark,
//! and a fixture living inside one of them cannot be read by the other.
//!
//! Two corpora, for two purposes. The **vendored subset** is small enough to live in the repository, so
//! the test suite needs no network. The **reference corpus** is fetched by one documented command and
//! checked against a pinned digest, because thirty-five megabytes of somebody else's prose does not
//! belong in a source repository — and because a corpus that drifts silently invalidates every number
//! ever recorded against it.
//!
//! Provenance, licence and attribution: `corpus/README.md`.

use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

/// The vendored subset, embedded so a test needs neither network nor a working directory.
const SMOKE: &str = include_str!("../../../corpus/smoke.tsv");

/// Where the fetched corpus lands, relative to the repository root.
const REFERENCE_FILE: &str = "corpus/reference.tsv";

/// The digest of the fetched corpus, as `scripts/fetch-corpus.py` produced it.
///
/// Pinned rather than recomputed: the point is to notice when the file is *not* what the recorded
/// numbers were measured against.
pub const REFERENCE_DIGEST: &str =
    "f85e3748907f2a7b9873e317d3d325d1ab8757e521d473f41c3cc618ce14b196";

/// How many documents the fetched corpus holds.
pub const REFERENCE_DOCUMENTS: usize = 1_905;

/// One document of the corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Stable across fetches, and prefixed with the language so a mixed corpus stays traceable.
    pub id: String,
    /// The Wikipedia language code the document came from.
    pub language: String,
    /// The article title.
    pub title: String,
    /// The article body, with its paragraphs intact.
    pub text: String,
}

impl Document {
    /// Read one document from one line of the corpus format.
    ///
    /// Returns `None` for anything that is not four tab-separated fields. A line the loader cannot read
    /// is a damaged corpus, and guessing at it would silently change what the benchmark measured.
    #[must_use]
    pub fn from_line(line: &str) -> Option<Self> {
        let mut fields = line.split('\t');
        let (id, language, title, text) = (
            fields.next()?,
            fields.next()?,
            fields.next()?,
            fields.next()?,
        );
        if fields.next().is_some() {
            return None;
        }
        Some(Self {
            id: id.to_owned(),
            language: language.to_owned(),
            title: unescape(title),
            text: unescape(text),
        })
    }
}

/// Undo the escaping the fetcher applies to the two characters the line format uses.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => {}
        }
    }
    out
}

/// The vendored subset: four languages, enough documents for a test and too few for a benchmark.
#[must_use]
pub fn smoke() -> Vec<Document> {
    parse(SMOKE)
}

/// The fetched reference corpus, verified against [`REFERENCE_DIGEST`].
///
/// # Errors
///
/// [`CorpusError::Missing`] when the file is not there, naming the command that fetches it —
/// a benchmark that fails with "no such file" and no instruction is a benchmark nobody runs.
/// [`CorpusError::Changed`] when the digest does not match, because measuring against a different
/// corpus than the recorded numbers came from is worse than not measuring.
pub fn reference(repository_root: impl AsRef<Path>) -> Result<Vec<Document>, CorpusError> {
    let path = repository_root.as_ref().join(REFERENCE_FILE);
    let raw =
        std::fs::read_to_string(&path).map_err(|_| CorpusError::Missing { path: path.clone() })?;

    let digest = Sha256::digest(raw.as_bytes());
    let found = digest.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
        out
    });
    if found != REFERENCE_DIGEST {
        return Err(CorpusError::Changed { path, found });
    }
    Ok(parse(&raw))
}

/// Every readable document in a corpus file.
fn parse(raw: &str) -> Vec<Document> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .filter_map(Document::from_line)
        .collect()
}

/// The reference corpus could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CorpusError {
    /// The corpus has not been fetched.
    #[error(
        "the reference corpus is not at {}: run `just corpus` to fetch it",
        path.display()
    )]
    Missing {
        /// Where it was looked for.
        path: PathBuf,
    },

    /// The corpus is not the one the recorded numbers were measured against.
    #[error(
        "the reference corpus at {} has digest {found}, not the pinned {}",
        path.display(),
        REFERENCE_DIGEST
    )]
    Changed {
        /// The file that does not match.
        path: PathBuf,
        /// What it hashes to now.
        found: String,
    },
}
