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
    ///
    /// Built through `Url::from_file_path` rather than by formatting the path, because a path is not a
    /// URL. `euledb://C:\\Users\\x` is not a URI — the drive letter becomes the authority and the
    /// backslashes are not separators — which is how the first version passed on Unix and failed on
    /// Windows. Going through the file-URL form gets the drive letter, the separators and the
    /// percent-encoding right, and only the scheme is substituted afterwards.
    pub(crate) fn uri(path: &std::path::Path) -> String {
        // Absolutised first: a file URL cannot be built from a relative path, and a store rooted at a
        // relative path is a legitimate thing for a caller to ask for.
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        match Url::from_file_path(&absolute) {
            Ok(file) => format!("{SCHEME}://{}", &file[url::Position::BeforeHost..]),
            // Unreachable for an absolute path, and a lossy URI is more useful than a panic: the format
            // will reject it with a message naming the path.
            Err(()) => format!("{SCHEME}://{}", absolute.display()),
        }
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
        // The same two steps the format's own local provider takes, and for the same reason: only
        // `to_file_path` knows what a Windows drive letter means, and only `from_absolute_path` knows
        // how to spell it as an object path. Since the scheme is the only difference, the URL is put
        // back into its file form first — `set_scheme` cannot do it, because the URL specification
        // forbids moving between a special scheme and a non-special one.
        let as_file = Url::parse(&format!("file://{}", &url[url::Position::BeforeHost..]));
        if let Ok(file) = as_file
            && let Ok(path) = file.to_file_path()
            && let Ok(object_path) = Path::from_absolute_path(&path)
        {
            return Ok(object_path);
        }
        Path::from_url_path(url.path()).map_err(|err| {
            lance::Error::invalid_input(format!("not a usable path in '{url}': {err}"))
        })
    }
}
