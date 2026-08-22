//! The key hierarchy: a passphrase wraps a data key, and a wrong passphrase gets nothing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use euledb_storage::{DataKeyId, Error, Keyring, KeyringError};

/// A passphrase of the kind a person actually types, not `"password"`.
const PASSPHRASE: &str = "korrektes-pferd-batterie-heftklammer";

#[test]
fn a_keyring_round_trips_through_its_keyfile() {
    let created = Keyring::create(PASSPHRASE).expect("creating a keyring must succeed");
    let keyfile = created.to_keyfile();

    let opened = Keyring::open(&keyfile, PASSPHRASE).expect("the right passphrase must open it");

    assert_eq!(
        opened.data_key(opened.current_data_key_id()),
        created.data_key(created.current_data_key_id()),
        "the data key that came back is not the one that was wrapped",
    );
}

#[test]
fn the_wrong_passphrase_fails_closed_with_a_distinct_error() {
    let keyfile = Keyring::create(PASSPHRASE)
        .expect("creating a keyring must succeed")
        .to_keyfile();

    let error = Keyring::open(&keyfile, "korrektes-pferd-batterie-heftklammern")
        .expect_err("one character wrong must not open the keyring");

    assert!(
        matches!(error, Error::Keyring(KeyringError::WrongPassphrase)),
        "a wrong passphrase must be its own error a caller can react to, got: {error:?}",
    );
}

#[test]
fn a_tampered_keyfile_fails_closed() {
    let mut keyfile = Keyring::create(PASSPHRASE)
        .expect("creating a keyring must succeed")
        .to_keyfile();

    // Flip a bit in the wrapped key. Authenticated encryption exists precisely so that this is
    // detected rather than producing a plausible-looking wrong key.
    let last = keyfile.len() - 1;
    keyfile[last] ^= 0b0000_0001;

    let error = Keyring::open(&keyfile, PASSPHRASE).expect_err("a tampered keyfile must not open");
    assert!(
        matches!(error, Error::Keyring(KeyringError::WrongPassphrase)),
        "tampering must fail closed, got: {error:?}",
    );
}

#[test]
fn a_truncated_keyfile_is_rejected_as_malformed() {
    let keyfile = Keyring::create(PASSPHRASE)
        .expect("creating a keyring must succeed")
        .to_keyfile();

    let error = Keyring::open(&keyfile[..keyfile.len() / 2], PASSPHRASE)
        .expect_err("half a keyfile is not a keyfile");
    assert!(
        matches!(error, Error::Keyring(KeyringError::MalformedKeyfile { .. })),
        "a truncated keyfile is malformed, not a wrong passphrase — a caller needs to tell those \
         apart to know whether to re-prompt. Got: {error:?}",
    );
}

#[test]
fn the_same_passphrase_yields_different_data_keys() {
    // The data key is generated, never derived from the passphrase. That is what makes it rotatable:
    // if it were derived, rotating would mean changing the passphrase and rewriting every byte.
    let first = Keyring::create(PASSPHRASE).expect("create");
    let second = Keyring::create(PASSPHRASE).expect("create");

    assert_ne!(
        first.data_key(first.current_data_key_id()),
        second.data_key(second.current_data_key_id()),
        "two databases with the same passphrase share a data key, so neither can be rotated \
         independently and one passphrase compromise exposes both",
    );
}

/// The keyfile layout, which is a persisted contract rather than an implementation detail. A test that
/// knows these offsets is the point: if they move, every existing database stops opening, so the change
/// has to be deliberate and versioned.
const VERSION_AT: usize = 0;
const SALT_RANGE: std::ops::Range<usize> = 1..17;
const NONCE_RANGE: std::ops::Range<usize> = 17..29;
/// A keyfile holding exactly one data key. The format is variable-length now — rotation adds 36 bytes
/// per key — so what is pinned is the size of the one-key case and where the fixed fields sit.
const ONE_KEY_KEYFILE_LEN: usize = 87;

#[test]
fn the_keyfile_layout_is_what_every_existing_database_expects() {
    let keyfile = Keyring::create(PASSPHRASE).expect("create").to_keyfile();

    assert_eq!(
        keyfile.len(),
        ONE_KEY_KEYFILE_LEN,
        "the one-key keyfile length changed, which means no database written by an earlier build opens",
    );
    assert_eq!(
        keyfile[VERSION_AT], 2,
        "the format version byte changed without the reader being taught the new one",
    );
}

#[test]
fn the_salt_differs_between_keyrings() {
    // A shared salt means one precomputation attacks every database using that passphrase. Comparing
    // whole keyfiles would NOT catch a fixed salt, because the nonce and the wrapped key differ anyway
    // — a mutation that fixed the salt passed against that weaker assertion.
    let first = Keyring::create(PASSPHRASE).expect("create").to_keyfile();
    let second = Keyring::create(PASSPHRASE).expect("create").to_keyfile();

    assert_ne!(
        first[SALT_RANGE], second[SALT_RANGE],
        "two keyrings share a salt, so one precomputation attacks both",
    );
}

#[test]
fn the_nonce_differs_between_keyrings() {
    // Same key never sees the same nonce twice. Here each keyring has its own key too, so a repeat
    // would not be immediately catastrophic — but a fixed nonce is a habit that becomes catastrophic
    // the moment a key is reused, and the data path will reuse one.
    let first = Keyring::create(PASSPHRASE).expect("create").to_keyfile();
    let second = Keyring::create(PASSPHRASE).expect("create").to_keyfile();

    assert_ne!(
        first[NONCE_RANGE], second[NONCE_RANGE],
        "two keyrings share a nonce, which is a habit that becomes catastrophic on key reuse",
    );
}

#[test]
fn a_keyfile_with_trailing_bytes_is_rejected_for_its_length() {
    // Not merely rejected: rejected FOR THE RIGHT REASON. Without the length check the extra bytes
    // would still be caught further in, by the wrapped-key conversion, with a message pointing at the
    // wrong thing — so the mutation that removed the length check passed until this asserted the reason.
    let mut keyfile = Keyring::create(PASSPHRASE).expect("create").to_keyfile();
    keyfile.extend_from_slice(b"appended");

    let error = Keyring::open(&keyfile, PASSPHRASE).expect_err("extra bytes are not a keyfile");
    assert!(
        matches!(error, Error::Keyring(KeyringError::MalformedKeyfile { reason }) if reason == "length is not a whole number of keys"
        ),
        "a keyfile of the wrong length must say so rather than blaming the passphrase, got: {error:?}",
    );
}

#[test]
fn a_keyring_does_not_print_its_key_material() {
    let keyring = Keyring::create(PASSPHRASE).expect("create");
    let rendered = format!("{keyring:?}");

    let key = keyring
        .data_key(keyring.current_data_key_id())
        .expect("the current key is in the keyring");
    let leaked = key
        .windows(4)
        .any(|window| rendered.contains(&format!("{window:?}")[1..]));
    assert!(
        !leaked,
        "the debug rendering contains key material, which is how a key ends up in a log: {rendered}",
    );
    assert!(
        !rendered.is_empty(),
        "a redacted debug rendering still has to say what the value is",
    );
}

#[test]
fn rotating_the_data_key_adds_one_and_keeps_the_old() {
    // Rotation must not discard the old key: data already written is sealed under it, and the criterion
    // forbids rewriting that payload. Discarding it would make the old rows unreadable, which is data
    // loss dressed up as a security measure.
    let mut keyring = Keyring::create(PASSPHRASE).expect("create");
    let before = keyring.current_data_key_id();
    let old_key = keyring
        .data_key(before)
        .expect("the key a fresh keyring is created with")
        .to_owned();

    let after = keyring.rotate_data_key().expect("rotating must succeed");

    assert_ne!(before, after, "rotation returned the id it started from");
    assert_eq!(
        keyring.current_data_key_id(),
        after,
        "the keyring did not adopt the new key as current",
    );
    assert_ne!(
        keyring
            .data_key(after)
            .expect("the new key is in the keyring")
            .to_owned(),
        old_key,
        "the new current key is the old one, so nothing rotated",
    );
    assert!(
        keyring.data_key(before).is_some(),
        "the previous key is gone, so everything written under it is unreadable",
    );
}

#[test]
fn a_rotated_keyring_round_trips_through_its_keyfile() {
    let mut keyring = Keyring::create(PASSPHRASE).expect("create");
    let first = keyring.current_data_key_id();
    let first_key = keyring
        .data_key(first)
        .expect("the first key is there")
        .to_owned();
    let second = keyring.rotate_data_key().expect("rotate");
    let second_key = keyring
        .data_key(second)
        .expect("the new key is there")
        .to_owned();

    let reopened =
        Keyring::open(&keyring.to_keyfile(), PASSPHRASE).expect("the passphrase must open it");

    assert_eq!(
        reopened.current_data_key_id(),
        second,
        "the current key did not survive"
    );
    assert_eq!(
        reopened.data_key(first).map(<[u8; 32]>::to_owned),
        Some(first_key),
        "the retired key did not survive, so earlier data is now unreadable",
    );
    assert_eq!(
        reopened.data_key(second).map(<[u8; 32]>::to_owned),
        Some(second_key),
        "the current key did not survive",
    );
}

#[test]
fn changing_the_passphrase_keeps_every_data_key() {
    // The other half of the criterion, and the cheap half: re-wrapping under a new key-encryption key
    // touches no payload at all, because the data keys themselves do not change.
    let mut keyring = Keyring::create(PASSPHRASE).expect("create");
    keyring.rotate_data_key().expect("rotate");
    let keys: Vec<[u8; 32]> = keyring
        .data_key_ids()
        .map(|id| keyring.data_key(id).expect("present").to_owned())
        .collect();

    let replacement = "neue-passphrase-mit-genug-entropie";
    keyring
        .change_passphrase(replacement)
        .expect("changing the passphrase must succeed");
    let keyfile = keyring.to_keyfile();

    let reopened = Keyring::open(&keyfile, replacement).expect("the new passphrase must open it");
    let after: Vec<[u8; 32]> = reopened
        .data_key_ids()
        .map(|id| reopened.data_key(id).expect("present").to_owned())
        .collect();
    assert_eq!(
        after, keys,
        "changing the passphrase changed the data keys, which would make every stored byte unreadable",
    );

    assert!(
        matches!(
            Keyring::open(&keyfile, PASSPHRASE),
            Err(Error::Keyring(KeyringError::WrongPassphrase))
        ),
        "the old passphrase still opens the keyring, so nothing was actually rotated",
    );
}

#[test]
fn a_key_that_was_never_in_the_keyring_is_not_found() {
    let keyring = Keyring::create(PASSPHRASE).expect("create");
    let absent = DataKeyId::from(9_999);
    assert!(
        keyring.data_key(absent).is_none(),
        "the keyring claims to hold a key it never generated",
    );
}

#[test]
fn changing_the_passphrase_takes_a_fresh_salt() {
    // Reusing the salt would let a precomputation built against the old passphrase carry straight over
    // to the new one, which defeats most of the point of changing it. Nothing else in the suite noticed
    // when the new salt was removed.
    let mut keyring = Keyring::create(PASSPHRASE).expect("create");
    let before = keyring.to_keyfile();

    keyring
        .change_passphrase("neue-passphrase-mit-genug-entropie")
        .expect("changing the passphrase must succeed");
    let after = keyring.to_keyfile();

    assert_ne!(
        before[SALT_RANGE], after[SALT_RANGE],
        "the salt survived a passphrase change, so a precomputation against the old one still applies",
    );
    assert_ne!(
        before[NONCE_RANGE], after[NONCE_RANGE],
        "the nonce survived a re-wrap under a new key, which is nonce reuse",
    );
}
