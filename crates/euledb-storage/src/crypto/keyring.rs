//! The keyring, and the keyfile it is persisted as.

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Nonce, Payload};
use argon2::{Algorithm, Argon2, Params, Version};

use super::secret::{KEY_LEN, SecretKey};

/// Keyfile format version, written first so a future format can be recognised rather than misread.
const VERSION: u8 = 1;

/// Argon2id salt length. Sixteen bytes is the length the RFC recommends and the crate's own default.
const SALT_LEN: usize = 16;

/// AES-GCM nonce length in bytes.
const NONCE_LEN: usize = 12;

/// AES-GCM authentication tag length in bytes.
const TAG_LEN: usize = 16;

/// Bytes a well-formed keyfile takes: version, salt, nonce, then the wrapped key and its tag.
const KEYFILE_LEN: usize = 1 + SALT_LEN + NONCE_LEN + KEY_LEN + TAG_LEN;

/// Bound into the wrapping as associated data, so a keyfile from another version or another product
/// cannot be replayed into this one — the tag covers it even though it is not secret.
const CONTEXT: &[u8] = b"euledb-keyfile-v1";

/// The keys a database is opened with.
///
/// Holds the data-encryption key in memory and knows how to write itself down in wrapped form. The key
/// is zeroized when this is dropped.
#[derive(Debug, Clone)]
pub struct Keyring {
    data_key: SecretKey,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    wrapped: [u8; KEY_LEN + TAG_LEN],
}

impl Keyring {
    /// Create a keyring for a new database.
    ///
    /// Generates a data-encryption key, derives a key-encryption key from the passphrase, and wraps the
    /// former with the latter. The salt is fresh, so two databases with the same passphrase share
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns [`KeyringError::Random`] if the platform's random source fails, and
    /// [`KeyringError::Derivation`] if key derivation does — neither is recoverable by retrying, and
    /// neither should be papered over with a weaker fallback.
    pub fn create(passphrase: &str) -> Result<Self, KeyringError> {
        let data_key = SecretKey::generate().map_err(|_| KeyringError::Random)?;
        let mut salt = [0_u8; SALT_LEN];
        let mut nonce = [0_u8; NONCE_LEN];
        getrandom::fill(&mut salt).map_err(|_| KeyringError::Random)?;
        getrandom::fill(&mut nonce).map_err(|_| KeyringError::Random)?;

        let kek = derive_key_encryption_key(passphrase, &salt)?;
        let sealed = cipher(&kek)?
            .encrypt(
                &nonce_of(&nonce),
                Payload {
                    msg: data_key.expose(),
                    aad: CONTEXT,
                },
            )
            .map_err(|_| KeyringError::Wrapping)?;

        let wrapped: [u8; KEY_LEN + TAG_LEN] =
            sealed.try_into().map_err(|_| KeyringError::Wrapping)?;

        Ok(Self {
            data_key,
            salt,
            nonce,
            wrapped,
        })
    }

    /// Open a keyring from its keyfile.
    ///
    /// # Errors
    ///
    /// [`KeyringError::MalformedKeyfile`] when the bytes are not a keyfile at all, and
    /// [`KeyringError::WrongPassphrase`] when they are but the passphrase does not unwrap them. The two
    /// are separate because a caller reacts differently: re-prompt for one, do not for the other.
    pub fn open(keyfile: &[u8], passphrase: &str) -> Result<Self, KeyringError> {
        if keyfile.len() != KEYFILE_LEN {
            return Err(KeyringError::MalformedKeyfile {
                reason: "wrong length",
            });
        }
        let (version, rest) = keyfile.split_at(1);
        if version[0] != VERSION {
            return Err(KeyringError::UnsupportedVersion { found: version[0] });
        }
        let (salt, rest) = rest.split_at(SALT_LEN);
        let (nonce, wrapped) = rest.split_at(NONCE_LEN);

        let salt: [u8; SALT_LEN] = salt
            .try_into()
            .map_err(|_| KeyringError::MalformedKeyfile {
                reason: "salt is not the expected length",
            })?;
        let nonce: [u8; NONCE_LEN] =
            nonce
                .try_into()
                .map_err(|_| KeyringError::MalformedKeyfile {
                    reason: "nonce is not the expected length",
                })?;
        let wrapped: [u8; KEY_LEN + TAG_LEN] =
            wrapped
                .try_into()
                .map_err(|_| KeyringError::MalformedKeyfile {
                    reason: "wrapped key is not the expected length",
                })?;

        let kek = derive_key_encryption_key(passphrase, &salt)?;
        // A wrong passphrase and a tampered keyfile are the same failure here, and deliberately so:
        // distinguishing them would tell an attacker which half they got right.
        let opened = cipher(&kek)?
            .decrypt(
                &nonce_of(&nonce),
                Payload {
                    msg: &wrapped,
                    aad: CONTEXT,
                },
            )
            .map_err(|_| KeyringError::WrongPassphrase)?;

        let bytes: [u8; KEY_LEN] = opened
            .try_into()
            .map_err(|_| KeyringError::WrongPassphrase)?;

        Ok(Self {
            data_key: SecretKey::new(bytes),
            salt,
            nonce,
            wrapped,
        })
    }

    /// The keyring in the form that is written to disk.
    ///
    /// Contains no secret: the salt and nonce are public by design, and the data key appears only
    /// wrapped under the key derived from the passphrase.
    #[must_use]
    pub fn to_keyfile(&self) -> Vec<u8> {
        let mut keyfile = Vec::with_capacity(KEYFILE_LEN);
        keyfile.push(VERSION);
        keyfile.extend_from_slice(&self.salt);
        keyfile.extend_from_slice(&self.nonce);
        keyfile.extend_from_slice(&self.wrapped);
        keyfile
    }

    /// The data-encryption key, for the layer that encrypts table data.
    ///
    /// Exposed because the data path needs it and lives in this crate. It is not re-exported beyond
    /// the storage boundary.
    #[must_use]
    pub fn data_key_bytes(&self) -> &[u8; KEY_LEN] {
        self.data_key.expose()
    }

    /// A frame that seals with this keyring's data key.
    ///
    /// The one place the data key is put to work. Everything else here only wraps and unwraps it.
    pub(crate) fn frame(&self, block_size: super::BlockSize) -> super::BlockFrame {
        super::BlockFrame::new(self.data_key.clone(), block_size)
    }
}

/// Derive the key-encryption key from the passphrase.
///
/// Argon2id, not Argon2i or Argon2d: it is the variant the RFC recommends, resisting both side-channel
/// and GPU-parallel attackers. The parameters are the crate's own defaults, which track that guidance —
/// hardcoding tuned values here would mean re-tuning them by hand as hardware changes, and getting it
/// wrong in the quiet direction.
fn derive_key_encryption_key(
    passphrase: &str,
    salt: &[u8; SALT_LEN],
) -> Result<SecretKey, KeyringError> {
    let mut key = [0_u8; KEY_LEN];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|_| KeyringError::Derivation)?;
    Ok(SecretKey::new(key))
}

/// The nonce as the cipher wants it.
///
/// The length is a compile-time constant that matches the cipher's, so this conversion cannot fail —
/// which is why the array type is the argument rather than a slice, and why there is no error path.
fn nonce_of(bytes: &[u8; NONCE_LEN]) -> Nonce<Aes256Gcm> {
    Nonce::<Aes256Gcm>::from(*bytes)
}

const _: () = assert!(
    NONCE_LEN == 12,
    "the nonce constant and the cipher's nonce size have to agree, and nonce_of relies on it"
);

/// The cipher used for wrapping, built from a key.
fn cipher(key: &SecretKey) -> Result<Aes256Gcm, KeyringError> {
    Aes256Gcm::new_from_slice(key.expose()).map_err(|_| KeyringError::Wrapping)
}

/// Something went wrong with the key material.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyringError {
    /// The passphrase does not unwrap this keyfile, or the keyfile was altered.
    ///
    /// One variant for both on purpose: telling them apart would reveal which half an attacker got
    /// right.
    #[error("the passphrase does not open this database")]
    WrongPassphrase,

    /// The bytes are not a keyfile.
    #[error("this is not a keyfile: {reason}")]
    MalformedKeyfile {
        /// What was wrong with it. Never contains key material.
        reason: &'static str,
    },

    /// A keyfile from a format version this build does not know.
    #[error("keyfile format version {found} is not supported by this build")]
    UnsupportedVersion {
        /// The version found in the file.
        found: u8,
    },

    /// The platform's random source failed.
    #[error("the operating system's random source failed, so no key could be generated")]
    Random,

    /// Key derivation failed.
    #[error("deriving a key from the passphrase failed")]
    Derivation,

    /// Wrapping or unwrapping the data key failed for a reason other than a wrong passphrase.
    #[error("the data key could not be wrapped")]
    Wrapping,
}
