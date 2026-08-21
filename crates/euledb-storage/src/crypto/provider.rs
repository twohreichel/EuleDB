//! Registers the encrypting store under its own URI scheme.
//!
//! **Why a scheme and not the format's wrapper hook.** The hook exists and is called, and it is not on
//! the path the data files take: `lance_io::object_store::ObjectStore::create` dispatches on the URI
//! scheme, and the `"file"` branch writes through `tokio::fs` without ever touching the `object_store`
//! trait. Measured — the wrapper saw `list` and the manifest `put`, no data-file write, and the row text
//! sat on disk in the clear. See `docs/adr/ADR-002-where-encryption-sits.md` § Amendment.
//!
//! Under a scheme that is not `file`, `is_local()` is false, `create` takes the `ObjectWriter` branch,
//! and every byte goes through the trait. The cost is that the format's local fast paths — the direct
//! writer, `copy_file`, `remove_dir_all` — are given up. That is the trade the amendment names.

use std::collections::HashMap;
use std::sync::Arc;

use lance::io::{ObjectStore as LanceObjectStore, ObjectStoreParams, ObjectStoreRegistry};
use object_store::local::LocalFileSystem;
use object_store::path::Path;
use url::Url;

use super::frame::BlockFrame;
use super::store::EncryptingObjectStore;

/// The URI scheme an encrypted database is addressed by.
///
/// Deliberately not `file`: that is the whole mechanism. It is also a marker in any log or error a
/// caller sees, which says plainly which path the bytes took.
pub(crate) const SCHEME: &str = "euledb";

/// Number of concurrent I/O operations the store may have outstanding.
///
/// The format's own local default. Not tuned here — tuning it belongs with a measurement.
const IO_PARALLELISM: usize = 8;

/// How often a failed download is retried before giving up.
const DOWNLOAD_RETRIES: usize = 3;

/// Builds the encrypting store for the [`SCHEME`] scheme.
#[derive(Debug)]
pub(crate) struct EncryptingProvider {
    frame: BlockFrame,
}

impl EncryptingProvider {
    /// A provider sealing with the given frame.
    pub(crate) const fn new(frame: BlockFrame) -> Self {
        Self { frame }
    }

    /// A registry that resolves [`SCHEME`] to this provider.
    ///
    /// Its own registry rather than the process-wide default: two databases open at once have different
    /// keys, and a shared registry would hand one database's cipher to the other.
    pub(crate) fn registry(frame: BlockFrame) -> Arc<ObjectStoreRegistry> {
        let registry = Arc::new(ObjectStoreRegistry::default());
        registry.insert(SCHEME, Arc::new(Self::new(frame)));
        registry
    }

    /// The URI an object under this scheme is addressed by.
    pub(crate) fn uri(path: &std::path::Path) -> String {
        format!("{SCHEME}://{}", path.display())
    }
}

#[async_trait::async_trait]
impl lance_io::object_store::ObjectStoreProvider for EncryptingProvider {
    async fn new_store(
        &self,
        base_path: Url,
        params: &ObjectStoreParams,
    ) -> lance::Result<LanceObjectStore> {
        // Rooted at the filesystem root, because the path carried in the URI is absolute.
        let local = Arc::new(LocalFileSystem::new());
        let encrypting = Arc::new(EncryptingObjectStore::new(local, self.frame.clone()));
        let options: Option<&HashMap<String, String>> = None;
        Ok(LanceObjectStore::new(
            encrypting,
            base_path,
            params.block_size,
            // No wrapper: this store IS the wrapper, and stacking one on top would seal twice.
            None,
            false,
            false,
            IO_PARALLELISM,
            DOWNLOAD_RETRIES,
            options,
        ))
    }

    fn extract_path(&self, url: &Url) -> lance::Result<Path> {
        // The inner store is rooted at `/`, so an absolute filesystem path minus its leading slash is
        // exactly what it wants.
        Path::from_url_path(url.path()).map_err(|err| {
            lance::Error::invalid_input(format!("not a usable path in '{url}': {err}"))
        })
    }
}
