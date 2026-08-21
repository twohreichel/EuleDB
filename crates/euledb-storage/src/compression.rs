//! How a table's columns are compressed on disk.
//!
//! The settings here are chosen from a measurement rather than from documentation, and the numbers are
//! reproducible with `cargo run --example measure_encoding`. Two of them decided the defaults:
//!
//! - Leaving the choice to the format compresses a repetitive multilingual corpus to about a third of
//!   its size with no code of ours, which is why **no own string encoder exists** — but the resulting
//!   size varies by more than 20 % between runs on identical input.
//! - Declaring zstd explicitly is byte-stable across runs AND about 15 % smaller than the best of
//!   those runs.
//!
//! Reproducibility is the deciding argument. A stored size that moves on its own cannot be compared
//! against a later one, which makes every size a benchmark records meaningless.

use std::collections::HashMap;

use arrow_schema::{Field, Schema};

/// The Arrow field-metadata keys the format reads its encoding configuration from.
const COMPRESSION_KEY: &str = "lance-encoding:compression";
const LEVEL_KEY: &str = "lance-encoding:compression-level";

/// A zstd compression level.
///
/// A newtype rather than a bare integer because the range is not obvious and the failure is silent:
/// zstd accepts 1 to 22, and a value outside that is a configuration mistake worth catching at the
/// boundary rather than at the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZstdLevel(u8);

impl ZstdLevel {
    /// The lowest level zstd defines.
    pub const FASTEST: Self = Self(1);

    /// The highest level zstd defines.
    pub const SMALLEST: Self = Self(22);

    /// The level this project uses unless a table asks for another.
    ///
    /// Measured on a 20 000-row multilingual corpus: level 1 produced 649 029 bytes and level 22's
    /// neighbourhood around 637 640 — **under 2 % smaller for several times the compression work**.
    /// The supported platforms include machines with four cores doing inference on the same cores, so
    /// the cheapest level that gets essentially all of the benefit is the honest default.
    ///
    /// Worth knowing before tuning: size is **not** monotonic in the level here. Level 3, zstd's own
    /// default, measured *larger* than level 1.
    pub const DEFAULT: Self = Self::FASTEST;

    /// Build a level, rejecting anything outside the range zstd defines.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidZstdLevel`] for `0` or for anything above 22.
    pub const fn new(level: u8) -> Result<Self, InvalidZstdLevel> {
        if level >= Self::FASTEST.0 && level <= Self::SMALLEST.0 {
            Ok(Self(level))
        } else {
            Err(InvalidZstdLevel { given: level })
        }
    }

    /// The level as zstd names it.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl Default for ZstdLevel {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A compression level outside the range zstd defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("zstd levels run from 1 to 22, and {given} is outside that")]
pub struct InvalidZstdLevel {
    /// The value that was rejected.
    pub given: u8,
}

/// How a table's columns are compressed.
///
/// Set once, when the table is created. Changing it afterwards would mean rewriting the data, so it is
/// deliberately not adjustable on an existing table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// zstd at the given level, applied to every column.
    ///
    /// The default, because it is both smaller and reproducible — see the module documentation.
    Zstd(ZstdLevel),

    /// No compression at all.
    ///
    /// Useful for measuring what compression is buying on a particular corpus, and for a table whose
    /// values are already compressed. Otherwise it is a poor trade: the measured corpus took 4.2 times
    /// the space this way.
    None,
}

impl Compression {
    /// zstd at a chosen level.
    #[must_use]
    pub const fn zstd(level: ZstdLevel) -> Self {
        Self::Zstd(level)
    }

    /// Store the columns uncompressed.
    #[must_use]
    pub const fn none() -> Self {
        Self::None
    }

    /// The field metadata that carries this setting to the encoder.
    fn metadata(self) -> HashMap<String, String> {
        match self {
            Self::Zstd(level) => HashMap::from([
                (COMPRESSION_KEY.to_owned(), "zstd".to_owned()),
                (LEVEL_KEY.to_owned(), level.get().to_string()),
            ]),
            Self::None => HashMap::from([(COMPRESSION_KEY.to_owned(), "none".to_owned())]),
        }
    }

    /// Restate a schema with this compression attached to every field.
    ///
    /// Applied to every column rather than only to the text ones, because the measurement said so:
    /// declaring it everywhere came out smaller than declaring it on the string columns alone
    /// (647 813 against 672 302 bytes), and it is the variant that is byte-stable across runs.
    pub(crate) fn applied_to(self, schema: &Schema) -> Schema {
        let metadata = self.metadata();
        let fields: Vec<Field> = schema
            .fields()
            .iter()
            .map(|field| {
                let mut merged = field.metadata().clone();
                merged.extend(metadata.clone());
                field.as_ref().clone().with_metadata(merged)
            })
            .collect();
        Schema::new_with_metadata(fields, schema.metadata().clone())
    }

    /// Take the encoding keys back off a schema.
    ///
    /// A caller who wrote a batch and reads it back has to get *their* schema, not one decorated with
    /// this crate's storage configuration. Leaving the keys on would put the encoding into every
    /// consumer's data and break the simplest equality a caller can write.
    pub(crate) fn stripped_from(schema: &Schema) -> Schema {
        let fields: Vec<Field> = schema
            .fields()
            .iter()
            .map(|field| {
                let mut metadata = field.metadata().clone();
                metadata.remove(COMPRESSION_KEY);
                metadata.remove(LEVEL_KEY);
                field.as_ref().clone().with_metadata(metadata)
            })
            .collect();
        Schema::new_with_metadata(fields, schema.metadata().clone())
    }
}

impl Default for Compression {
    fn default() -> Self {
        Self::Zstd(ZstdLevel::DEFAULT)
    }
}
