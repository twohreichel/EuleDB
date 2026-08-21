//! Block framing, so an encrypted object can still be read in ranges.
//!
//! AES-GCM is not seekable: a file sealed as one message has to be decrypted whole before any of it can
//! be read, which would destroy the random access the on-disk format was chosen for. So an object is a
//! header followed by fixed-size blocks, each sealed on its own:
//!
//! ```text
//! ┌────────── header ──────────┬───── block 0 ─────┬───── block 1 ─────┬ ...
//! │ magic(4) ver(1) block_sz(4)│ nonce(12) ct+tag  │ nonce(12) ct+tag  │
//! └────────────────────────────┴───────────────────┴───────────────────┘
//! ```
//!
//! Three decisions in that layout, each load-bearing:
//!
//! - **Each block carries its own random nonce**, rather than one derived from the object's path. A
//!   path-derived nonce would be smaller and prettier, and it would break the moment the format renames
//!   or copies an object — which it does, on every commit. Random 96-bit nonces are sound under one key
//!   up to about 2^32 blocks, which at the default block size is far past any plausible local database.
//! - **The block index AND whether it is the final block are authenticated data.** The index stops two
//!   blocks being exchanged — each is individually valid, so nothing else would notice. The final-block
//!   marker stops truncation: a reader that reaches the end without having seen a block marked final
//!   knows bytes are missing, which the length alone cannot tell it. Both come from the shape
//!   established streaming-AEAD designs use.
//! - **The block size is written into the header.** It becomes part of the layout the moment anything is
//!   stored, so a reader has to learn it from the object rather than from the build that reads it.
//!
//! What this does NOT protect against, stated because it matters: an attacker who replaces one whole
//! object with another object sealed under the same key. The object's identity is deliberately not
//! authenticated, because binding it would break rename and copy. Detecting that substitution belongs
//! to the layer that knows which objects should exist.

use std::ops::Range;

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Nonce, Payload};

use super::secret::SecretKey;

/// Marks an object as framed by this crate, so a plaintext file is not mistaken for a sealed one.
const MAGIC: [u8; 4] = *b"EULE";

/// Framing version. A reader that does not know a version refuses rather than guesses.
const VERSION: u8 = 1;

/// `magic` + `version` + `block_size`.
const HEADER_LEN: usize = 4 + 1 + 4;

/// AES-GCM nonce length in bytes.
const NONCE_LEN: usize = 12;

/// AES-GCM authentication tag length in bytes.
const TAG_LEN: usize = 16;

/// Overhead each block pays: its nonce and its tag.
const BLOCK_OVERHEAD: usize = NONCE_LEN + TAG_LEN;

/// The plaintext bytes per block.
///
/// A newtype because the value is part of the on-disk layout: once an object exists it cannot change,
/// and a value that is not a power of two in a sane range turns every offset calculation into a puzzle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BlockSize(usize);

impl BlockSize {
    /// The smallest supported block. Below this the per-block overhead dominates.
    pub(crate) const MIN: usize = 64;

    /// The largest supported block. Above this a small read amplifies absurdly.
    pub(crate) const MAX: usize = 1 << 20;

    /// The default, 64 KiB.
    ///
    /// Chosen by measurement — `cargo run --example measure_framing` reports the trade-off between read
    /// amplification and per-block overhead across the range.
    pub(crate) const DEFAULT: Self = Self(64 * 1024);

    /// Build a block size, refusing anything that is not a power of two in range.
    pub(crate) const fn new(bytes: usize) -> Result<Self, FrameError> {
        if bytes >= Self::MIN && bytes <= Self::MAX && bytes.is_power_of_two() {
            Ok(Self(bytes))
        } else {
            Err(FrameError::UnsupportedBlockSize { given: bytes })
        }
    }

    /// The size in bytes.
    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

impl Default for BlockSize {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Seals and opens objects a block at a time.
#[derive(Debug, Clone)]
pub(crate) struct BlockFrame {
    key: SecretKey,
    block_size: BlockSize,
}

impl BlockFrame {
    /// A frame for one key and one block size.
    pub(crate) const fn new(key: SecretKey, block_size: BlockSize) -> Self {
        Self { key, block_size }
    }

    /// Bytes the header takes.
    pub(crate) const fn header_len(&self) -> usize {
        HEADER_LEN
    }

    /// Bytes a full block takes once sealed.
    pub(crate) const fn sealed_block_len(&self) -> usize {
        self.block_size.get() + BLOCK_OVERHEAD
    }

    /// How many bytes `plaintext_len` plaintext bytes occupy once sealed.
    pub(crate) const fn ciphertext_len(&self, plaintext_len: u64) -> u64 {
        let block = self.block_size.get() as u64;
        let full = plaintext_len / block;
        let remainder = plaintext_len % block;
        let mut total = HEADER_LEN as u64 + full * self.sealed_block_len() as u64;
        // A remainder gets a partial final block. No remainder still gets one — empty and marked final
        // — so that an object truncated to its header is not mistaken for an empty object.
        total += remainder + BLOCK_OVERHEAD as u64;
        total
    }

    /// The inverse: how much plaintext a sealed object of this size holds.
    pub(crate) fn plaintext_len(&self, ciphertext_len: u64) -> Result<u64, FrameError> {
        let header = HEADER_LEN as u64;
        if ciphertext_len < header {
            return Err(FrameError::Truncated);
        }
        let body = ciphertext_len - header;
        let stride = self.sealed_block_len() as u64;
        let full = body / stride;
        let remainder = body % stride;
        // Every object ends with a partial final block, which is at least the per-block overhead.
        if remainder < BLOCK_OVERHEAD as u64 {
            return Err(FrameError::Truncated);
        }
        Ok(full * self.block_size.get() as u64 + remainder - BLOCK_OVERHEAD as u64)
    }

    /// The ciphertext bytes that have to be fetched to serve a plaintext range.
    ///
    /// Whole blocks, because a block is the smallest thing that can be authenticated. The caller reads
    /// exactly this span and passes it back to [`Self::open_span`].
    pub(crate) fn ciphertext_span(
        &self,
        plaintext: Range<u64>,
        plaintext_total: u64,
    ) -> Range<u64> {
        let block = self.block_size.get() as u64;
        let first = plaintext.start / block;
        let last = plaintext.end.div_ceil(block).max(first + 1);
        let stride = self.sealed_block_len() as u64;
        let start = HEADER_LEN as u64 + first * stride;
        // Clamped, because the final block is shorter than a full one: an unclamped span reaches past
        // the end of the object for any read near it, and the caller would slice out of bounds.
        let end = (HEADER_LEN as u64 + last * stride).min(self.ciphertext_len(plaintext_total));
        start..end.max(start)
    }

    /// Seal a whole object.
    ///
    /// # Errors
    ///
    /// [`FrameError::Random`] if the platform's random source fails, [`FrameError::Cipher`] if sealing
    /// does — neither is recoverable by retrying.
    pub(crate) fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, FrameError> {
        let cipher = self.cipher()?;
        let mut out = Vec::with_capacity(
            usize::try_from(self.ciphertext_len(plaintext.len() as u64)).unwrap_or(0),
        );
        out.extend_from_slice(&MAGIC);
        out.push(VERSION);
        out.extend_from_slice(
            &u32::try_from(self.block_size.get())
                .map_err(|_| FrameError::UnsupportedBlockSize {
                    given: self.block_size.get(),
                })?
                .to_le_bytes(),
        );

        // Every object ends with a block SHORTER than a full one — empty when the plaintext is an exact
        // multiple of the block size, including when it is empty. That is what makes "final" readable
        // from a block's length alone, which in turn is what makes a truncation that removes whole
        // blocks detectable. Inferring it from the length without this rule is wrong for exactly the
        // lengths that are multiples of the block size, and those are the common case.
        let blocks = self.block_size.get();
        let mut chunks: Vec<&[u8]> = plaintext.chunks(blocks).collect();
        if plaintext.len() % blocks == 0 {
            chunks.push(&[]);
        }
        let last = chunks.len() - 1;
        for (index, chunk) in chunks.into_iter().enumerate() {
            let mut nonce = [0_u8; NONCE_LEN];
            getrandom::fill(&mut nonce).map_err(|_| FrameError::Random)?;
            let sealed = cipher
                .encrypt(
                    &Nonce::<Aes256Gcm>::from(nonce),
                    Payload {
                        msg: chunk,
                        aad: &Self::associated_data(index as u64, index == last),
                    },
                )
                .map_err(|_| FrameError::Cipher)?;
            out.extend_from_slice(&nonce);
            out.extend_from_slice(&sealed);
        }
        Ok(out)
    }

    /// Open a whole object.
    ///
    /// # Errors
    ///
    /// Fails closed on anything unexpected, and **returns no plaintext at all** when any block fails —
    /// a partial answer from an authenticated format would be worse than none.
    pub(crate) fn open(&self, ciphertext: &[u8]) -> Result<Vec<u8>, FrameError> {
        let plaintext_len = self.plaintext_len(ciphertext.len() as u64)?;
        self.read_header(ciphertext)?;
        // open_span takes the BODY and the span it occupies, so the header is left behind here rather
        // than handed on — passing the whole object made every round trip look truncated.
        self.open_span(
            &ciphertext[HEADER_LEN..],
            HEADER_LEN as u64..(ciphertext.len() as u64),
            0..plaintext_len,
        )
    }

    /// Open the blocks in `span` and return the `wanted` plaintext range from them.
    ///
    /// `span` is what [`Self::ciphertext_span`] returned, and `ciphertext` is exactly those bytes.
    ///
    /// # Errors
    ///
    /// [`FrameError::Authentication`] naming the block that failed, [`FrameError::Truncated`] if the
    /// span does not hold whole blocks, [`FrameError::RangeOutsideObject`] if `wanted` is not inside it.
    pub(crate) fn open_span(
        &self,
        ciphertext: &[u8],
        span: Range<u64>,
        wanted: Range<u64>,
    ) -> Result<Vec<u8>, FrameError> {
        let cipher = self.cipher()?;
        let stride = self.sealed_block_len();
        let header = HEADER_LEN as u64;
        if span.start < header || (span.start - header) % stride as u64 != 0 {
            return Err(FrameError::Truncated);
        }
        let first_block = (span.start - header) / stride as u64;
        let block = self.block_size.get() as u64;

        let mut plaintext =
            Vec::with_capacity(usize::try_from(wanted.end - wanted.start).unwrap_or(0));
        let mut offset = first_block * block;
        for (position, sealed) in ciphertext.chunks(stride).enumerate() {
            // Strictly less, not less-or-equal: a block of exactly the overhead is the valid empty
            // final block that every exact-multiple object ends with. Rejecting it made every object
            // whose length is a multiple of the block size unreadable.
            if sealed.len() < BLOCK_OVERHEAD {
                return Err(FrameError::Truncated);
            }
            let index = first_block + position as u64;
            let (nonce, body) = sealed.split_at(NONCE_LEN);
            let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| FrameError::Truncated)?;
            let opened = cipher
                .decrypt(
                    &Nonce::<Aes256Gcm>::from(nonce),
                    Payload {
                        msg: body,
                        // Whether this is the final block is known from its length: only the last one
                        // is shorter than a full block. A truncation that removes whole blocks
                        // therefore fails here, because the block it stops at was not sealed as final.
                        aad: &Self::associated_data(index, sealed.len() < stride),
                    },
                )
                .map_err(|_| FrameError::Authentication { block: index })?;

            // Keep only the part of this block the caller asked for.
            let block_start = offset;
            let block_end = offset + opened.len() as u64;
            offset = block_end;
            let from = wanted.start.max(block_start);
            let to = wanted.end.min(block_end);
            if from < to {
                let lo = usize::try_from(from - block_start).unwrap_or(0);
                let hi = usize::try_from(to - block_start).unwrap_or(0);
                plaintext.extend_from_slice(&opened[lo..hi]);
            }
        }

        if plaintext.len() as u64 != wanted.end - wanted.start {
            return Err(FrameError::RangeOutsideObject);
        }
        Ok(plaintext)
    }

    /// Check the header and return the block size it declares.
    ///
    /// # Errors
    ///
    /// [`FrameError::NotFramed`] when the magic is absent, [`FrameError::UnsupportedVersion`] for a
    /// version this build does not know, and [`FrameError::BlockSizeMismatch`] when the object was
    /// written with a different block size than this frame uses.
    pub(crate) fn read_header(&self, ciphertext: &[u8]) -> Result<BlockSize, FrameError> {
        if ciphertext.len() < HEADER_LEN {
            return Err(FrameError::Truncated);
        }
        if ciphertext[..4] != MAGIC {
            return Err(FrameError::NotFramed);
        }
        if ciphertext[4] != VERSION {
            return Err(FrameError::UnsupportedVersion {
                found: ciphertext[4],
            });
        }
        let declared =
            u32::from_le_bytes([ciphertext[5], ciphertext[6], ciphertext[7], ciphertext[8]])
                as usize;
        if declared != self.block_size.get() {
            return Err(FrameError::BlockSizeMismatch {
                declared,
                configured: self.block_size.get(),
            });
        }
        BlockSize::new(declared)
    }

    /// The authenticated data for a block: the framing version and the block's index.
    ///
    /// The index is what makes reordering detectable. The version is there so a future framing cannot
    /// be confused with this one even if a reader is careless about the header.
    fn associated_data(index: u64, final_block: bool) -> [u8; 10] {
        let mut aad = [0_u8; 10];
        aad[0] = VERSION;
        aad[1..9].copy_from_slice(&index.to_le_bytes());
        aad[9] = u8::from(final_block);
        aad
    }

    fn cipher(&self) -> Result<Aes256Gcm, FrameError> {
        Aes256Gcm::new_from_slice(self.key.expose()).map_err(|_| FrameError::Cipher)
    }
}

/// Something went wrong framing or unframing an object.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FrameError {
    /// A block did not authenticate: it was altered, reordered, or sealed under another key.
    #[error("block {block} did not authenticate, so no plaintext is returned for this read")]
    Authentication {
        /// Index of the first block that failed.
        block: u64,
    },

    /// The bytes end mid-structure.
    #[error("the object is truncated")]
    Truncated,

    /// The object is not framed by this crate at all.
    #[error("this object is not encrypted by EuleDB")]
    NotFramed,

    /// A framing version this build does not know.
    #[error("framing version {found} is not supported by this build")]
    UnsupportedVersion {
        /// The version found in the header.
        found: u8,
    },

    /// The object was written with a different block size.
    #[error(
        "the object declares a block size of {declared} but this database is configured for {configured}"
    )]
    BlockSizeMismatch {
        /// What the object says.
        declared: usize,
        /// What this frame was built with.
        configured: usize,
    },

    /// A block size that is not a power of two between the supported bounds.
    #[error("a block size must be a power of two between 64 and 1048576, and {given} is not")]
    UnsupportedBlockSize {
        /// The value that was refused.
        given: usize,
    },

    /// The requested range is not inside the object.
    #[error("the requested range is not inside this object")]
    RangeOutsideObject,

    /// The platform's random source failed.
    #[error("the operating system's random source failed, so no nonce could be generated")]
    Random,

    /// The cipher refused, for a reason other than authentication.
    #[error("the cipher could not be used")]
    Cipher,
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "in a test an unwrap IS the assertion"
    )]

    use super::{BlockFrame, BlockSize, FrameError};
    use crate::crypto::secret::SecretKey;

    /// A small block size, so a test corpus of a few hundred bytes spans several blocks.
    const SMALL: usize = 64;

    fn frame() -> BlockFrame {
        BlockFrame::new(
            SecretKey::new([7_u8; 32]),
            BlockSize::new(SMALL).expect("64 is a valid block size"),
        )
    }

    /// Plaintext of a given length, varied enough that a misplaced byte is visible.
    fn plaintext(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect()
    }

    /// Lengths that exercise every boundary the framing has: empty, sub-block, exact multiples, and
    /// one either side of each.
    const LENGTHS: [usize; 9] = [
        0,
        1,
        SMALL - 1,
        SMALL,
        SMALL + 1,
        2 * SMALL,
        2 * SMALL + 1,
        200,
        1000,
    ];

    #[test]
    fn sealing_then_opening_returns_the_plaintext_for_every_boundary_length() {
        let frame = frame();
        for len in LENGTHS {
            let original = plaintext(len);
            let sealed = frame.seal(&original).expect("sealing must succeed");
            let opened = frame.open(&sealed).expect("opening must succeed");
            assert_eq!(opened, original, "round trip lost data at length {len}");
        }
    }

    #[test]
    fn the_ciphertext_length_is_predicted_exactly_and_inverts() {
        let frame = frame();
        for len in LENGTHS {
            let sealed = frame.seal(&plaintext(len)).expect("seal");
            let predicted = frame.ciphertext_len(len as u64);
            assert_eq!(
                sealed.len() as u64,
                predicted,
                "the predicted ciphertext length is wrong at plaintext length {len}, which means a \
                 reader would compute the wrong offsets",
            );
            assert_eq!(
                frame.plaintext_len(predicted).expect("invert"),
                len as u64,
                "the length mapping does not invert at {len}",
            );
        }
    }

    #[test]
    fn a_flipped_bit_anywhere_fails_closed() {
        let frame = frame();
        let sealed = frame.seal(&plaintext(200)).expect("seal");
        for position in 0..sealed.len() {
            let mut tampered = sealed.clone();
            tampered[position] ^= 0b0000_0001;
            let result = frame.open(&tampered);
            assert!(
                result.is_err(),
                "flipping bit 0 of byte {position} still opened, so that byte is unauthenticated",
            );
        }
    }

    #[test]
    fn swapping_two_blocks_fails_closed() {
        // Both blocks are validly sealed under the same key, so only binding the block index into the
        // authenticated data stops them being reordered. Without it a reader would accept the swap.
        let frame = frame();
        let sealed = frame.seal(&plaintext(3 * SMALL)).expect("seal");
        let stride = frame.sealed_block_len();
        let header = frame.header_len();

        let mut swapped = sealed.clone();
        let (first, second) = (header, header + stride);
        for offset in 0..stride {
            swapped.swap(first + offset, second + offset);
        }

        assert!(
            matches!(
                frame.open(&swapped),
                Err(FrameError::Authentication { block: 0 })
            ),
            "two whole blocks were exchanged and the reader accepted it",
        );
    }

    #[test]
    fn a_truncated_object_fails_closed() {
        let frame = frame();
        let sealed = frame.seal(&plaintext(200)).expect("seal");
        for cut in [
            1_usize,
            frame.header_len(),
            sealed.len() / 2,
            sealed.len() - 1,
        ] {
            assert!(
                frame.open(&sealed[..cut]).is_err(),
                "a ciphertext truncated to {cut} bytes still opened",
            );
        }
    }

    #[test]
    fn sealing_the_same_plaintext_twice_gives_different_ciphertext() {
        // Each block carries its own random nonce. Identical output would mean a fixed nonce, and a
        // repeated nonce under one key is what breaks GCM completely rather than gradually.
        let frame = frame();
        let first = frame.seal(&plaintext(200)).expect("seal");
        let second = frame.seal(&plaintext(200)).expect("seal");
        assert_ne!(
            first, second,
            "the nonce is not random, so it will eventually repeat"
        );
    }

    #[test]
    fn a_range_read_returns_the_same_bytes_as_the_whole_read_sliced() {
        let frame = frame();
        let original = plaintext(500);
        let sealed = frame.seal(&original).expect("seal");

        for range in [0..1_u64, 0..64, 10..70, 63..65, 100..400, 499..500, 0..500] {
            let ciphertext_span = frame.ciphertext_span(range.clone(), original.len() as u64);
            let start = usize::try_from(ciphertext_span.start).expect("fits");
            let end = usize::try_from(ciphertext_span.end).expect("fits");
            let opened = frame
                .open_span(&sealed[start..end], ciphertext_span, range.clone())
                .expect("a range read must succeed");

            let wanted = &original[range.start as usize..range.end as usize];
            assert_eq!(opened, wanted, "range {range:?} came back wrong");
        }
    }

    #[test]
    fn a_range_reaching_past_the_object_is_an_error_not_a_short_answer() {
        // A short answer is the dangerous outcome: a caller that asked for 1000 bytes and silently got
        // 500 has no way to notice, and every offset it computes afterwards is wrong.
        let frame = frame();
        let original = plaintext(500);
        let sealed = frame.seal(&original).expect("seal");

        let wanted = 400..1000_u64;
        let span = frame.ciphertext_span(wanted.clone(), original.len() as u64);
        let start = usize::try_from(span.start).expect("fits");
        let end = usize::try_from(span.end).expect("fits");

        assert!(
            matches!(
                frame.open_span(&sealed[start..end], span, wanted),
                Err(FrameError::RangeOutsideObject)
            ),
            "a range reaching past the end returned data instead of refusing",
        );
    }

    #[test]
    fn a_range_read_fetches_only_the_blocks_it_needs() {
        // The whole point of framing. If a one-byte read pulled the entire object, the format's random
        // access would be gone and the design would have failed silently.
        let frame = frame();
        let sealed_len = frame.ciphertext_len(10_000);
        let span = frame.ciphertext_span(5_000..5_001, 10_000);
        assert_eq!(
            span.end - span.start,
            frame.sealed_block_len() as u64,
            "a single-byte read spans {} bytes instead of one block",
            span.end - span.start,
        );
        assert!(span.end <= sealed_len, "the span reaches past the object");
    }

    #[test]
    fn an_object_written_with_another_block_size_is_refused() {
        // The block size is part of the layout, so a reader configured differently must refuse rather
        // than compute offsets that happen to parse. The header carries it for exactly this check.
        let written = frame();
        let sealed = written.seal(&plaintext(200)).expect("seal");

        let other = BlockFrame::new(
            SecretKey::new([7_u8; 32]),
            BlockSize::new(128).expect("128 is a valid block size"),
        );

        assert!(
            matches!(
                other.read_header(&sealed),
                Err(FrameError::BlockSizeMismatch {
                    declared: SMALL,
                    configured: 128
                })
            ),
            "a frame configured for a different block size opened an object it cannot address",
        );
    }

    #[test]
    fn a_block_forged_with_the_wrong_final_marker_is_refused() {
        // The structural rule — every object ends with a block shorter than a full one — already
        // catches truncation, so the final marker in the authenticated data is not what fails first
        // there. This forges the one case where it IS the only thing standing: a block re-sealed under
        // the right key, at the right index, claiming to be final when it is not. A streaming reader,
        // which does not know the total length up front, has nothing else to go on.
        use aes_gcm::Aes256Gcm;
        use aes_gcm::aead::{Aead, KeyInit, Nonce, Payload};

        let frame = frame();
        let original = plaintext(3 * SMALL);
        let mut sealed = frame.seal(&original).expect("seal");

        let header = frame.header_len();
        let stride = frame.sealed_block_len();
        let key = SecretKey::new([7_u8; 32]);
        let cipher = Aes256Gcm::new_from_slice(key.expose()).expect("key length is correct");

        // Re-seal block 1 with everything the same except the final marker.
        let nonce = [42_u8; 12];
        let forged = cipher
            .encrypt(
                &Nonce::<Aes256Gcm>::from(nonce),
                Payload {
                    msg: &original[SMALL..2 * SMALL],
                    aad: &BlockFrame::associated_data(1, true),
                },
            )
            .expect("sealing must succeed");
        let at = header + stride;
        sealed[at..at + 12].copy_from_slice(&nonce);
        sealed[at + 12..at + stride].copy_from_slice(&forged);

        assert!(
            matches!(
                frame.open(&sealed),
                Err(FrameError::Authentication { block: 1 })
            ),
            "a block claiming to be final when it is not was accepted",
        );
    }

    #[test]
    fn a_block_size_outside_the_supported_range_is_refused() {
        for refused in [0_usize, 1, 63, 65, 1 << 30] {
            assert!(
                BlockSize::new(refused).is_err(),
                "{refused} is not a supported block size and must be refused",
            );
        }
        for accepted in [64_usize, 4096, 65_536, 1 << 20] {
            assert!(
                BlockSize::new(accepted).is_ok(),
                "{accepted} is a power of two in range and must be accepted",
            );
        }
    }
}
