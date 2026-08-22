//! The encrypting layer the on-disk format writes through.
//!
//! The format is unaware of it. It asks its object store for bytes and ranges, and this sits in between,
//! sealing on the way down and opening on the way up (ADR-002). Two consequences worth stating:
//!
//! - **Sizes are translated.** The format asks how large an object is and then computes offsets from
//!   that answer. Reporting the ciphertext size would make every offset it derives wrong, so `head` and
//!   `list` report the *plaintext* size.
//! - **A range read stays a range read.** A requested plaintext range is mapped to the blocks covering
//!   it, and only those are fetched. That is the whole reason for the framing.

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream::{BoxStream, StreamExt};
use object_store::path::Path;
use object_store::{
    CopyOptions, Error as StoreError, GetOptions, GetRange, GetResult, GetResultPayload,
    ListResult, MultipartUpload, ObjectMeta, ObjectStore, PutMultipartOptions, PutOptions,
    PutPayload, PutResult, Result as StoreResult, UploadPart,
};

use super::frame::{BlockFrame, FrameError};

/// Wraps an object store so that everything passing through it is sealed.
#[derive(Debug)]
pub(crate) struct EncryptingObjectStore {
    inner: Arc<dyn ObjectStore>,
    frame: BlockFrame,
}

impl EncryptingObjectStore {
    /// Wrap a store.
    pub(crate) fn new(inner: Arc<dyn ObjectStore>, frame: BlockFrame) -> Self {
        Self { inner, frame }
    }

    /// Restate an object's metadata in plaintext terms.
    fn as_plaintext(&self, mut meta: ObjectMeta) -> StoreResult<ObjectMeta> {
        meta.size = self
            .frame
            .plaintext_len(meta.size)
            .map_err(|err| crypto_error(&meta.location, err))?;
        Ok(meta)
    }

    /// Resolve a requested range against the plaintext length.
    fn wanted_range(requested: Option<&GetRange>, plaintext_len: u64) -> Range<u64> {
        match requested {
            None => 0..plaintext_len,
            Some(GetRange::Bounded(range)) => {
                range.start.min(plaintext_len)..range.end.min(plaintext_len)
            }
            Some(GetRange::Offset(from)) => (*from).min(plaintext_len)..plaintext_len,
            Some(GetRange::Suffix(len)) => plaintext_len.saturating_sub(*len)..plaintext_len,
        }
    }
}

impl std::fmt::Display for EncryptingObjectStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Encrypting({})", self.inner)
    }
}

/// Present a framing failure as a store failure, without leaking key material into the message.
fn crypto_error(location: &Path, err: FrameError) -> StoreError {
    StoreError::Generic {
        store: "euledb-encrypted",
        source: Box::new(LocatedFrameError {
            location: location.to_string(),
            source: err,
        }),
    }
}

/// A framing failure with the object it happened on, which is what an operator needs to act.
#[derive(Debug, thiserror::Error)]
#[error("{location}: {source}")]
struct LocatedFrameError {
    location: String,
    source: FrameError,
}

#[async_trait]
impl ObjectStore for EncryptingObjectStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> StoreResult<PutResult> {
        let plaintext: Vec<u8> = payload
            .iter()
            .flat_map(|part| part.iter().copied())
            .collect();
        let sealed = self
            .frame
            .seal(&plaintext)
            .map_err(|err| crypto_error(location, err))?;
        self.inner
            .put_opts(location, PutPayload::from(sealed), opts)
            .await
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> StoreResult<Box<dyn MultipartUpload>> {
        let inner = self.inner.put_multipart_opts(location, opts).await?;
        Ok(Box::new(SealingUpload::new(
            inner,
            self.frame.clone(),
            location.clone(),
        )))
    }

    async fn get_opts(&self, location: &Path, options: GetOptions) -> StoreResult<GetResult> {
        // The ciphertext size first, because every offset below is derived from the plaintext size and
        // that is only knowable from it. Conditional options travel with this request so a precondition
        // is evaluated once, by the store that owns the object.
        let wants_metadata_only = options.head;
        let requested_range = options.range.clone();
        let probe = GetOptions {
            range: None,
            head: true,
            ..options
        };
        let head = self.inner.get_opts(location, probe).await?;
        let meta = self.as_plaintext(head.meta)?;
        let plaintext_len = meta.size;

        if wants_metadata_only {
            // A head request wants metadata, not bytes.
            return Ok(GetResult {
                payload: GetResultPayload::Stream(futures_util::stream::empty().boxed()),
                range: 0..0,
                attributes: head.attributes,
                meta,
            });
        }

        let wanted = Self::wanted_range(requested_range.as_ref(), plaintext_len);
        if wanted.start >= wanted.end {
            return Ok(GetResult {
                payload: GetResultPayload::Stream(futures_util::stream::empty().boxed()),
                range: wanted,
                attributes: head.attributes,
                meta,
            });
        }

        let span = self.frame.ciphertext_span(wanted.clone(), plaintext_len);
        // The header AND the block span in one call. Validating the header is what turns "block 0 did
        // not authenticate" into "this object is not encrypted by EuleDB" or "it declares a block size
        // of 4096 and this database is configured for 65536" — the difference between a diagnosis and a
        // shrug. get_ranges exists for exactly this: two ranges, one round trip.
        let header_span = 0..self.frame.header_len() as u64;
        let fetched = self
            .inner
            .get_ranges(location, &[header_span, span.clone()])
            .await?;
        let header = fetched.first().cloned().unwrap_or_default();
        // The header says which key sealed this object and at what block size, and both are needed to
        // open it. A rotated keyring reads an older object because the object names its own key.
        let framing = self
            .frame
            .read_header(&header)
            .map_err(|err| crypto_error(location, err))?;
        let sealed = fetched.get(1).cloned().unwrap_or_default();
        let plaintext = self
            .frame
            .open_span(&sealed, span, wanted.clone(), framing)
            .map_err(|err| crypto_error(location, err))?;

        Ok(GetResult {
            payload: GetResultPayload::Stream(
                futures_util::stream::once(async move { Ok(Bytes::from(plaintext)) }).boxed(),
            ),
            range: wanted,
            attributes: head.attributes,
            meta,
        })
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, StoreResult<Path>>,
    ) -> BoxStream<'static, StoreResult<Path>> {
        self.inner.delete_stream(locations)
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, StoreResult<ObjectMeta>> {
        let frame = self.frame.clone();
        self.inner
            .list(prefix)
            .map(move |entry| {
                entry.and_then(|mut meta| {
                    meta.size = frame
                        .plaintext_len(meta.size)
                        .map_err(|err| crypto_error(&meta.location, err))?;
                    Ok(meta)
                })
            })
            .boxed()
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> StoreResult<ListResult> {
        let mut listing = self.inner.list_with_delimiter(prefix).await?;
        listing.objects = listing
            .objects
            .into_iter()
            .map(|meta| self.as_plaintext(meta))
            .collect::<StoreResult<Vec<_>>>()?;
        Ok(listing)
    }

    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> StoreResult<()> {
        // Ciphertext copies verbatim, because nothing in a block is bound to the object's identity.
        // That is a deliberate property of the framing: the format renames and copies objects on every
        // commit, and a path-derived key would break both.
        self.inner.copy_opts(from, to, options).await
    }
}

/// A multipart upload that seals whole blocks as they become available.
///
/// The last block has to be shorter than a full one, and whether a block is last is only known when
/// the upload completes — so the tail is held back until then. Full blocks can be sealed immediately,
/// because a full block is never the last one.
#[derive(Debug)]
struct SealingUpload {
    inner: Box<dyn MultipartUpload>,
    frame: BlockFrame,
    location: Path,
    pending: Vec<u8>,
    next_index: u64,
    header_sent: bool,
}

impl SealingUpload {
    fn new(inner: Box<dyn MultipartUpload>, frame: BlockFrame, location: Path) -> Self {
        Self {
            inner,
            frame,
            location,
            pending: Vec::new(),
            next_index: 0,
            header_sent: false,
        }
    }

    /// Seal everything that is certainly not the final block, and hand it to the inner upload.
    fn drain_full_blocks(&mut self) -> StoreResult<Vec<u8>> {
        let mut out = Vec::new();
        if !self.header_sent {
            out.extend_from_slice(&self.frame.header());
            self.header_sent = true;
        }
        let block = self.frame.block_size();
        while self.pending.len() >= block {
            let chunk: Vec<u8> = self.pending.drain(..block).collect();
            let sealed = self
                .frame
                .seal_block(&chunk, self.next_index, false)
                .map_err(|err| crypto_error(&self.location, err))?;
            out.extend_from_slice(&sealed);
            self.next_index += 1;
        }
        Ok(out)
    }
}

#[async_trait]
impl MultipartUpload for SealingUpload {
    fn put_part(&mut self, data: PutPayload) -> UploadPart {
        self.pending
            .extend(data.iter().flat_map(|part| part.iter().copied()));
        match self.drain_full_blocks() {
            Ok(sealed) if sealed.is_empty() => Box::pin(async { Ok(()) }),
            Ok(sealed) => self.inner.put_part(PutPayload::from(sealed)),
            Err(err) => Box::pin(async move { Err(err) }),
        }
    }

    async fn complete(&mut self) -> StoreResult<PutResult> {
        let mut tail = self
            .drain_full_blocks()
            .map_err(|err| StoreError::Generic {
                store: "euledb-encrypted",
                source: Box::new(err),
            })?;
        let remaining: Vec<u8> = std::mem::take(&mut self.pending);
        let sealed = self
            .frame
            .seal_block(&remaining, self.next_index, true)
            .map_err(|err| crypto_error(&self.location, err))?;
        self.next_index += 1;
        tail.extend_from_slice(&sealed);
        self.inner.put_part(PutPayload::from(tail)).await?;
        self.inner.complete().await
    }

    async fn abort(&mut self) -> StoreResult<()> {
        self.inner.abort().await
    }
}
