//! The keyring, and the keyfile it is persisted as.

use std::collections::BTreeMap;

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Nonce, Payload};
use argon2::{Algorithm, Argon2, Params, Version};

use super::secret::{KEY_LEN, SecretKey};

/// Keyfile format version, written first so a future format can be recognised rather than misread.
///
/// Version 2 holds a *set* of data keys rather than one. Rotation adds a key and retires the previous
/// one without discarding it, because the payload it sealed is not rewritten.
const VERSION: u8 = 2;

/// Argon2id salt length. Sixteen bytes is the length the RFC recommends and the crate's own default.
const SALT_LEN: usize = 16;

/// AES-GCM nonce length in bytes.
const NONCE_LEN: usize = 12;

/// AES-GCM authentication tag length in bytes.
const TAG_LEN: usize = 16;

/// Bytes the fixed part of a keyfile takes: version, salt, nonce.
const PREFIX_LEN: usize = 1 + SALT_LEN + NONCE_LEN;

/// Bytes each key takes inside the sealed key set: its id and the key itself.
const ENTRY_LEN: usize = 4 + KEY_LEN;

/// Bytes the sealed key set's own preamble takes: the current id and the number of keys.
const SET_HEADER_LEN: usize = 4 + 2;

/// Bound into the wrapping as associated data, so a keyfile from another version or another product
/// cannot be replayed into this one — the tag covers it even though it is not secret.
const CONTEXT: &[u8] = b"euledb-keyfile-v2";

/// Identifies one data key within a keyring.
///
/// A newtype because it is written into every sealed object's header: it is an on-disk identifier, and
/// confusing it with a length or a count would be silent corruption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DataKeyId(u32);

impl DataKeyId {
    /// The id every keyring starts with.
    pub(crate) const FIRST: Self = Self(1);

    /// The id as it appears on disk.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The next id after this one, or `None` if the space is exhausted.
    const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

impl From<u32> for DataKeyId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for DataKeyId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// The keys a database is opened with.
///
/// Holds every data key the database has ever used, and knows which one new writes are sealed under.
/// Retired keys are kept because the payload they sealed is never rewritten — discarding one would make
/// the rows it protects unreadable, which is data loss wearing a security measure's clothes.
///
/// Key material is zeroized when this is dropped.
#[derive(Debug, Clone)]
pub struct Keyring {
    keys: BTreeMap<DataKeyId, SecretKey>,
    current: DataKeyId,
    /// The key derived from the passphrase, kept so that rotating a data key does not need the
    /// passphrase again — Argon2id is deliberately expensive, and asking for it on every write would
    /// push callers towards caching it themselves, less carefully.
    kek: SecretKey,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    /// The wrapped key set, recomputed whenever the set or the passphrase changes.
    ///
    /// Held rather than produced on demand so that writing the keyfile cannot fail. Every recomputation
    /// takes a **fresh nonce**: re-sealing a changed key set under the same key and nonce would be
    /// nonce reuse, which breaks GCM completely rather than gradually.
    sealed: Vec<u8>,
}

impl Keyring {
    /// Create a keyring for a new database.
    ///
    /// Generates one data key, derives a key-encryption key from the passphrase, and wraps the key set
    /// with it. The salt is fresh, so two databases with the same passphrase share nothing.
    ///
    /// # Errors
    ///
    /// [`KeyringError::Random`] if the platform's random source fails, [`KeyringError::Derivation`] if
    /// key derivation does. Neither is recoverable by retrying, and neither should be papered over with
    /// a weaker fallback.
    pub fn create(passphrase: &str) -> crate::Result<Self> {
        let mut keys = BTreeMap::new();
        keys.insert(
            DataKeyId::FIRST,
            SecretKey::generate().map_err(|_| KeyringError::Random)?,
        );
        let mut salt = [0_u8; SALT_LEN];
        getrandom::fill(&mut salt).map_err(|_| KeyringError::Random)?;
        let mut keyring = Self {
            keys,
            current: DataKeyId::FIRST,
            kek: derive_key_encryption_key(passphrase, &salt)?,
            salt,
            nonce: [0_u8; NONCE_LEN],
            sealed: Vec::new(),
        };
        keyring.reseal()?;
        Ok(keyring)
    }

    /// Open a keyring from its keyfile.
    ///
    /// # Errors
    ///
    /// [`KeyringError::MalformedKeyfile`] when the bytes are not a keyfile at all, and
    /// [`KeyringError::WrongPassphrase`] when they are but the passphrase does not unwrap them. The two
    /// are separate because a caller reacts differently: re-prompt for one, do not for the other.
    pub fn open(keyfile: &[u8], passphrase: &str) -> crate::Result<Self> {
        if keyfile.len() < PREFIX_LEN {
            return Err(KeyringError::MalformedKeyfile {
                reason: "shorter than a keyfile header",
            }
            .into());
        }
        let (version, rest) = keyfile.split_at(1);
        if version[0] != VERSION {
            return Err(KeyringError::UnsupportedVersion { found: version[0] }.into());
        }
        let (salt, rest) = rest.split_at(SALT_LEN);
        let (nonce, sealed) = rest.split_at(NONCE_LEN);

        // A variable-length keyfile cannot be checked against a single expected size, but its shape is
        // still fixed: a tag, a set header, and a whole number of key entries. Checking that here means
        // a damaged file is reported as damaged, instead of the tag failing and the caller being told
        // their passphrase is wrong — which would send them to re-type a passphrase that was correct.
        let smallest = SET_HEADER_LEN + ENTRY_LEN + TAG_LEN;
        if sealed.len() < smallest {
            return Err(KeyringError::MalformedKeyfile {
                reason: "too short to hold even one key",
            }
            .into());
        }
        if !(sealed.len() - TAG_LEN - SET_HEADER_LEN).is_multiple_of(ENTRY_LEN) {
            return Err(KeyringError::MalformedKeyfile {
                reason: "length is not a whole number of keys",
            }
            .into());
        }
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

        let kek = derive_key_encryption_key(passphrase, &salt)?;
        // A wrong passphrase and a tampered keyfile are the same failure here, and deliberately so:
        // distinguishing them would tell an attacker which half they got right.
        let opened = cipher(&kek)?
            .decrypt(
                &nonce_of(&nonce),
                Payload {
                    msg: sealed,
                    aad: CONTEXT,
                },
            )
            .map_err(|_| KeyringError::WrongPassphrase)?;

        let (current, keys) = decode_key_set(&opened)?;
        Ok(Self {
            keys,
            current,
            kek,
            salt,
            nonce,
            sealed: sealed.to_vec(),
        })
    }

    /// The keyring in the form that is written to disk.
    ///
    /// Contains no secret: the salt and nonce are public by design, and every data key appears only
    /// wrapped under the key derived from the passphrase.
    #[must_use]
    pub fn to_keyfile(&self) -> Vec<u8> {
        let mut keyfile = Vec::with_capacity(PREFIX_LEN + self.sealed.len());
        keyfile.push(VERSION);
        keyfile.extend_from_slice(&self.salt);
        keyfile.extend_from_slice(&self.nonce);
        keyfile.extend_from_slice(&self.sealed);
        keyfile
    }

    /// Retire the current data key and start sealing new writes under a fresh one.
    ///
    /// **No payload is rewritten.** Everything already stored stays sealed under the key that sealed it,
    /// and that key stays in the keyring so it stays readable. The cost is that a keyring grows
    /// by 36 bytes per rotation and that old data keeps whatever protection its key had.
    ///
    /// # Errors
    ///
    /// [`KeyringError::Random`] if the platform's random source fails, and
    /// [`KeyringError::KeySpaceExhausted`] after four billion rotations.
    pub fn rotate_data_key(&mut self) -> crate::Result<DataKeyId> {
        let next = self
            .keys
            .keys()
            .next_back()
            .copied()
            .unwrap_or(DataKeyId::FIRST)
            .next()
            .ok_or(KeyringError::KeySpaceExhausted)?;
        self.keys.insert(
            next,
            SecretKey::generate().map_err(|_| KeyringError::Random)?,
        );
        self.current = next;
        self.reseal()?;
        Ok(next)
    }

    /// Wrap the same key set under a key derived from a new passphrase.
    ///
    /// The data keys are untouched, so **not one stored byte changes** — which is what makes this the
    /// cheap half of rotation, and the right response to a leaked passphrase.
    ///
    /// # Errors
    ///
    /// [`KeyringError::Random`] if the platform's random source fails, [`KeyringError::Derivation`] if
    /// key derivation does.
    pub fn change_passphrase(&mut self, passphrase: &str) -> crate::Result<()> {
        // A new salt as well as a new key: reusing the salt would let a precomputation built against
        // the old passphrase carry over.
        getrandom::fill(&mut self.salt).map_err(|_| KeyringError::Random)?;
        self.kek = derive_key_encryption_key(passphrase, &self.salt)?;
        self.reseal().map_err(Into::into)
    }

    /// The key capability tags are signed with.
    ///
    /// Derived from the key-encryption key under its own context, so a token key and a data key can
    /// never be the same bytes — a token forgeable by anyone who can read a block header would be no
    /// gate at all.
    pub(crate) fn token_key(&self) -> [u8; KEY_LEN] {
        use hmac::{KeyInit as _, Mac as _};
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(self.kek.expose())
            .unwrap_or_else(|_| unreachable!("HMAC accepts a key of any length"));
        mac.update(b"euledb/token-key/v1");
        mac.finalize().into_bytes().into()
    }

    /// The id new writes are sealed under.
    #[must_use]
    pub fn current_data_key_id(&self) -> DataKeyId {
        self.current
    }

    /// Every id this keyring holds, oldest first.
    pub fn data_key_ids(&self) -> impl Iterator<Item = DataKeyId> + '_ {
        self.keys.keys().copied()
    }

    /// One data key, or `None` if this keyring never held it.
    #[must_use]
    pub fn data_key(&self, id: DataKeyId) -> Option<&[u8; KEY_LEN]> {
        self.keys.get(&id).map(SecretKey::expose)
    }

    /// A frame that seals with the current key and opens with any key this keyring holds.
    ///
    /// The one place the data keys are put to work. Everything else here only wraps and unwraps them.
    pub(crate) fn frame(&self, block_size: super::BlockSize) -> super::BlockFrame {
        super::BlockFrame::new(self.keys.clone(), self.current, block_size)
    }

    /// Wrap the current key set under the held key-encryption key, with a fresh nonce.
    fn reseal(&mut self) -> Result<(), KeyringError> {
        getrandom::fill(&mut self.nonce).map_err(|_| KeyringError::Random)?;
        let plaintext = encode_key_set(self.current, &self.keys);
        self.sealed = cipher(&self.kek)?
            .encrypt(
                &nonce_of(&self.nonce),
                Payload {
                    msg: &plaintext,
                    aad: CONTEXT,
                },
            )
            .map_err(|_| KeyringError::Wrapping)?;
        Ok(())
    }
}

/// Serialise the key set: the current id, the count, then each id and key.
fn encode_key_set(current: DataKeyId, keys: &BTreeMap<DataKeyId, SecretKey>) -> Vec<u8> {
    let mut out = Vec::with_capacity(SET_HEADER_LEN + keys.len() * ENTRY_LEN);
    out.extend_from_slice(&current.get().to_le_bytes());
    out.extend_from_slice(&u16::try_from(keys.len()).unwrap_or(u16::MAX).to_le_bytes());
    for (id, key) in keys {
        out.extend_from_slice(&id.get().to_le_bytes());
        out.extend_from_slice(key.expose());
    }
    out
}

/// The inverse of [`encode_key_set`].
fn decode_key_set(
    bytes: &[u8],
) -> Result<(DataKeyId, BTreeMap<DataKeyId, SecretKey>), KeyringError> {
    if bytes.len() < SET_HEADER_LEN {
        return Err(KeyringError::MalformedKeyfile {
            reason: "key set is shorter than its own header",
        });
    }
    let current = DataKeyId::from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
    let count = usize::from(u16::from_le_bytes([bytes[4], bytes[5]]));
    let entries = &bytes[SET_HEADER_LEN..];
    if entries.len() != count * ENTRY_LEN {
        return Err(KeyringError::MalformedKeyfile {
            reason: "key set does not hold the number of keys it declares",
        });
    }

    // as_chunks rather than chunks_exact: the chunk size is a constant, so each entry arrives as a
    // fixed-size array and every index below is checked at compile time instead of at runtime.
    let (entries, remainder) = entries.as_chunks::<ENTRY_LEN>();
    if !remainder.is_empty() {
        return Err(KeyringError::MalformedKeyfile {
            reason: "key set ends mid-entry",
        });
    }

    let mut keys = BTreeMap::new();
    for entry in entries {
        let (id, key) = entry.split_at(4);
        let id = DataKeyId::from(u32::from_le_bytes([id[0], id[1], id[2], id[3]]));
        let key: [u8; KEY_LEN] = key.try_into().map_err(|_| KeyringError::MalformedKeyfile {
            reason: "a key is not the expected length",
        })?;
        keys.insert(id, SecretKey::new(key));
    }
    if !keys.contains_key(&current) {
        return Err(KeyringError::MalformedKeyfile {
            reason: "the current key is not in the key set",
        });
    }
    Ok((current, keys))
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

    /// Four billion rotations is enough.
    #[error("the data-key id space is exhausted")]
    KeySpaceExhausted,

    /// The platform's random source failed.
    #[error("the operating system's random source failed, so no key could be generated")]
    Random,

    /// Key derivation failed.
    #[error("deriving a key from the passphrase failed")]
    Derivation,

    /// Wrapping or unwrapping the key set failed for a reason other than a wrong passphrase.
    #[error("the key set could not be wrapped")]
    Wrapping,
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
    )]

    use super::Keyring;

    /// Key separation, asserted from inside because it is not observable from outside.
    ///
    /// Signing tokens with the key-encryption key itself, or with a data key, would pass every behavioural
    /// test in the suite — the gate would still work. What it would cost is the property that reading a
    /// block header, or learning one data key, tells an attacker nothing about forging a token. That
    /// property has no observable behaviour, so it is asserted directly.
    #[test]
    fn the_token_key_is_not_any_key_that_touches_data() {
        let keyring = Keyring::create("korrektes-pferd-batterie-heftklammer").expect("keyring");
        let token_key = keyring.token_key();

        assert_ne!(
            &token_key,
            keyring.kek.expose(),
            "signing tokens with the key that wraps the key set would make one secret do two jobs",
        );
        for id in keyring.data_key_ids().collect::<Vec<_>>() {
            let data_key = keyring.data_key(id).expect("an id from the set resolves");
            assert_ne!(
                &token_key, data_key,
                "signing tokens with a data key would make a token forgeable by anyone holding it",
            );
        }
    }

    /// The same passphrase under a different salt is a different authority.
    #[test]
    fn two_keyrings_do_not_share_a_token_key() {
        let phrase = "korrektes-pferd-batterie-heftklammer";
        let one = Keyring::create(phrase).expect("keyring");
        let other = Keyring::create(phrase).expect("keyring");

        assert_ne!(
            one.token_key(),
            other.token_key(),
            "a token signed by one authority must not verify under another",
        );
    }
}
