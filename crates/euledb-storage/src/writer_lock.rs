//! The one-writer rule, enforced by an advisory lock on a file beside the tables.

use std::fs::File;
use std::path::{Path, PathBuf};

/// Name of the lock file, beside the tables rather than inside one.
///
/// Its own file because the lock has to exist before any table does, and because deleting a table must
/// not release the database.
const LOCK_FILE: &str = ".euledb-writer.lock";

/// Proof that this process holds the write role for one database.
///
/// The lock is **advisory** and held by the operating system for as long as the file handle lives — so
/// it is released when this value is dropped, and also when the process dies for any reason, including
/// being killed. That last property is why an advisory lock is the right tool rather than a marker file:
/// a marker left behind by a crashed writer would lock the database out permanently, and the first thing
/// anyone would learn is how to delete it.
#[derive(Debug)]
pub struct WriteLock {
    /// Held for its side effect. Dropping the handle releases the lock.
    _handle: File,
    root: PathBuf,
}

impl WriteLock {
    /// Take the write role for the database under `root`, or report who has it.
    ///
    /// Creates `root` if it does not exist, because a database is opened for writing before it contains
    /// anything.
    ///
    /// # Errors
    ///
    /// [`LockError::Busy`] when another writer holds the database — **immediately**, never after a wait.
    /// A local-first database that blocks indefinitely on a lock held by a process nobody can see is
    /// worse than one that says so. [`LockError::Unavailable`] when the lock file cannot be created or
    /// locked at all, which is a filesystem or permission problem rather than contention.
    pub fn acquire(root: impl AsRef<Path>) -> Result<Self, LockError> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root).map_err(|cause| LockError::Unavailable {
            root: root.clone(),
            cause: cause.to_string(),
        })?;
        let path = root.join(LOCK_FILE);
        let handle = File::options()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|cause| LockError::Unavailable {
                root: root.clone(),
                cause: cause.to_string(),
            })?;

        match handle.try_lock() {
            Ok(()) => Ok(Self {
                _handle: handle,
                root,
            }),
            // try_lock reports contention as a distinct kind, so a busy database and a broken filesystem
            // are not the same answer to the caller.
            Err(std::fs::TryLockError::WouldBlock) => Err(LockError::Busy { root }),
            Err(std::fs::TryLockError::Error(cause)) => Err(LockError::Unavailable {
                root,
                cause: cause.to_string(),
            }),
        }
    }

    /// The database this lock is held for.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// The write role could not be taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LockError {
    /// Another writer holds this database.
    #[error("another writer already holds the database at {}", root.display())]
    Busy {
        /// The database that is held.
        root: PathBuf,
    },

    /// The lock itself could not be established.
    #[error("the write lock for {} could not be taken: {cause}", root.display())]
    Unavailable {
        /// The database that was being opened.
        root: PathBuf,
        /// What the filesystem said. A string, because the caller cannot act on the io::Error's kind
        /// any differently and keeping it would put std's error in this crate's public API for nothing.
        cause: String,
    },
}
