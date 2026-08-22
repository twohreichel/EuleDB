//! An append-only record of what the database was asked to do, chained so a removal shows.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

/// The log's file name, beside the tables rather than inside one.
const LOG_FILE: &str = ".euledb-audit.log";

/// How long a link is. SHA-256, so 32 bytes.
const HASH_LEN: usize = 32;

/// One entry: what was asked, how it resolved, what it touched, and the link to the entry before it.
///
/// **What is deliberately absent:** the rows. An audit log that copies the data it describes is a second
/// copy of the database, and this one is not encrypted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    sequence: u64,
    previous: [u8; HASH_LEN],
    query: String,
    plan: String,
    rows: u64,
    hash: [u8; HASH_LEN],
}

impl AuditRecord {
    /// Where this record sits in the chain. Gapless, so a removed entry is visible as a jump.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// What was asked — the operation, its table, and the predicate where there was one.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// How it resolved, where the layer knows. Empty for an operation with nothing to resolve.
    #[must_use]
    pub fn plan(&self) -> &str {
        &self.plan
    }

    /// How many rows the operation affected or returned.
    #[must_use]
    pub const fn rows(&self) -> u64 {
        self.rows
    }

    /// This record's own link.
    #[must_use]
    pub const fn hash(&self) -> &[u8; HASH_LEN] {
        &self.hash
    }

    /// The hash this record claims to follow.
    #[must_use]
    pub const fn previous(&self) -> &[u8; HASH_LEN] {
        &self.previous
    }

    /// The link this record's content produces.
    ///
    /// Length-prefixed rather than concatenated, so the bytes hashed have exactly one reading: without
    /// the prefixes a query ending in what the next field begins with could be rearranged into a
    /// different record with the same hash.
    fn recompute(&self) -> [u8; HASH_LEN] {
        let mut digest = Sha256::new();
        digest.update(self.previous);
        digest.update(self.sequence.to_be_bytes());
        digest.update(
            u64::try_from(self.query.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        digest.update(self.query.as_bytes());
        digest.update(
            u64::try_from(self.plan.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        digest.update(self.plan.as_bytes());
        digest.update(self.rows.to_be_bytes());
        digest.finalize().into()
    }

    /// The record as one line: fields separated by tabs, the two free-form ones escaped.
    fn to_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            self.sequence,
            hex(&self.previous),
            hex(&self.hash),
            self.rows,
            escape(&self.query),
            escape(&self.plan),
        )
    }

    /// Read a record back from its line.
    fn from_line(line: &str) -> Result<Self, AuditError> {
        let malformed = |reason: &'static str| AuditError::Malformed { reason };
        let mut fields = line.split('\t');
        let mut next = |reason: &'static str| fields.next().ok_or_else(|| malformed(reason));

        let sequence = next("no sequence")?
            .parse()
            .map_err(|_| malformed("the sequence is not a number"))?;
        let previous =
            unhex(next("no previous hash")?).ok_or_else(|| malformed("bad previous hash"))?;
        let hash = unhex(next("no hash")?).ok_or_else(|| malformed("bad hash"))?;
        let rows = next("no row count")?
            .parse()
            .map_err(|_| malformed("the row count is not a number"))?;
        let query = unescape(next("no query")?);
        let plan = unescape(next("no plan")?);

        Ok(Self {
            sequence,
            previous,
            query,
            plan,
            rows,
            hash,
        })
    }
}

/// Every record in a log's contents.
///
/// Separate from [`AuditLog::records`] because an append has to parse what it read through its own
/// locked handle rather than by opening the file a second time.
fn parse(raw: &str) -> Result<Vec<AuditRecord>, AuditError> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .map(AuditRecord::from_line)
        .collect()
}

/// Escape the two characters the line format uses, and the escape itself.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}

/// Undo [`escape`]. A trailing lone backslash is dropped rather than treated as an error: it cannot be
/// produced by `escape`, so a line carrying one is already damaged and the chain will say so.
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
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => {}
        }
    }
    out
}

/// Bytes as lowercase hex.
fn hex(bytes: &[u8; HASH_LEN]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Hex back to bytes, or `None` if it is not exactly one hash.
fn unhex(text: &str) -> Option<[u8; HASH_LEN]> {
    if text.len() != HASH_LEN * 2 {
        return None;
    }
    let mut out = [0_u8; HASH_LEN];
    for (index, slot) in out.iter_mut().enumerate() {
        let pair = text.get(index * 2..index * 2 + 2)?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// An append-only, hash-chained log of the operations performed on one database.
///
/// **Where** — its own file beside the tables, so the lock it needs is not the database's write lock.
/// **Why its own lock** — many readers may hold a database at once and a read is an operation that gets
/// recorded, so readers serialise for the length of one append and nothing else.
#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    /// Point at the log of the database under `root`. The file need not exist yet.
    #[must_use]
    pub fn open(root: impl AsRef<Path>) -> Self {
        Self {
            path: root.as_ref().join(LOG_FILE),
        }
    }

    /// Append one record, taking a short exclusive lock on the log file alone.
    ///
    /// # Errors
    ///
    /// [`AuditError::Unavailable`] when the file cannot be opened, locked or written — which is a
    /// filesystem or permission problem, and the reason auditing is a tunable rather than mandatory.
    pub fn append(&self, query: &str, plan: &str, rows: u64) -> Result<(), AuditError> {
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&self.path)
            .map_err(|cause| AuditError::unavailable(&self.path, cause))?;
        file.lock()
            .map_err(|cause| AuditError::unavailable(&self.path, cause))?;

        // Read the tail under the lock, not before it: two handles that both decided their sequence
        // number before locking would write the same one.
        //
        // Read through THIS handle, not by opening the file again. A file lock is advisory on Unix and
        // **mandatory** on Windows, so a second handle reading the file this one has locked is refused
        // there — the four-platform matrix is what surfaced that, because on Unix it simply works.
        // Writes still go to the end regardless of where this leaves the read cursor, because the handle
        // was opened in append mode.
        let mut raw = String::new();
        file.seek(SeekFrom::Start(0))
            .map_err(|cause| AuditError::unavailable(&self.path, cause))?;
        file.read_to_string(&mut raw)
            .map_err(|cause| AuditError::unavailable(&self.path, cause))?;
        let existing = parse(&raw)?;
        let (sequence, previous) = existing
            .last()
            .map_or((0, [0_u8; HASH_LEN]), |last| (last.sequence + 1, last.hash));

        let mut record = AuditRecord {
            sequence,
            previous,
            query: query.to_owned(),
            plan: plan.to_owned(),
            rows,
            hash: [0_u8; HASH_LEN],
        };
        record.hash = record.recompute();

        file.write_all(record.to_line().as_bytes())
            .map_err(|cause| AuditError::unavailable(&self.path, cause))?;
        file.flush()
            .map_err(|cause| AuditError::unavailable(&self.path, cause))
    }

    /// Every record, in the order they were appended.
    ///
    /// # Errors
    ///
    /// [`AuditError::Unavailable`] when the file cannot be read, [`AuditError::Malformed`] when a line
    /// is not a record.
    pub fn records(&self) -> Result<Vec<AuditRecord>, AuditError> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(cause) => return Err(AuditError::unavailable(&self.path, cause)),
        };
        parse(&raw)
    }

    /// Whether the file exists at all.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}

/// The audit log could not be written or read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AuditError {
    /// The log file could not be opened, locked, read or written.
    #[error("the audit log at {path} is unavailable: {cause}")]
    Unavailable {
        /// The file that could not be used.
        path: String,
        /// What the filesystem said. A string, because a caller cannot act on the kind any differently
        /// and keeping it would put std's error in this crate's public API for nothing.
        cause: String,
    },

    /// A line in the log is not a record.
    #[error("the audit log holds a line that is not a record: {reason}")]
    Malformed {
        /// Which part of the line was wrong.
        reason: &'static str,
    },
}

impl AuditError {
    /// An unavailable log, with the path it was about.
    fn unavailable(path: &Path, cause: impl std::fmt::Display) -> Self {
        Self::Unavailable {
            path: path.display().to_string(),
            cause: cause.to_string(),
        }
    }
}
