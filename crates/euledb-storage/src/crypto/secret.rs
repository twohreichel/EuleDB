//! A key, held so that it does not outlive its use.

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Length of every key here, in bytes. AES-256 takes 32.
pub(crate) const KEY_LEN: usize = 32;

/// Thirty-two bytes of key material, wiped when dropped.
///
/// Two properties, and both are the reason this is a type rather than a `[u8; 32]`:
///
/// - **It is zeroized on drop.** A key left in freed memory is recoverable from a core dump or a swap
///   file long after the process that held it exited.
/// - **Its `Debug` shows nothing.** The most common way a key reaches a log is a struct deriving
///   `Debug` somewhere up the tree, and the author of that struct never thinking about it.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub(crate) struct SecretKey([u8; KEY_LEN]);

impl SecretKey {
    /// Take ownership of key material.
    pub(crate) const fn new(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// Generate a key from the operating system's random source.
    ///
    /// # Errors
    ///
    /// Returns the failure from the platform's random source, which is not something to paper over
    /// with a fallback: a key from a weakened source is worse than no key, because it looks like one.
    pub(crate) fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0_u8; KEY_LEN];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes))
    }

    /// The bytes, for handing to a cipher.
    pub(crate) const fn expose(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl std::fmt::Debug for SecretKey {
    /// Says that a key is there, never which one.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretKey(redacted)")
    }
}
