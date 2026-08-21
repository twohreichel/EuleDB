//! The key hierarchy: a passphrase wraps a data key, and a wrong passphrase gets nothing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use euledb_storage::{Keyring, KeyringError};

/// A passphrase of the kind a person actually types, not `"password"`.
const PASSPHRASE: &str = "korrektes-pferd-batterie-heftklammer";

#[test]
fn a_keyring_round_trips_through_its_keyfile() {
    let created = Keyring::create(PASSPHRASE).expect("creating a keyring must succeed");
    let keyfile = created.to_keyfile();

    let opened = Keyring::open(&keyfile, PASSPHRASE).expect("the right passphrase must open it");

    assert_eq!(
        opened.data_key_bytes(),
        created.data_key_bytes(),
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
        matches!(error, KeyringError::WrongPassphrase),
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
        matches!(error, KeyringError::WrongPassphrase),
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
        matches!(error, KeyringError::MalformedKeyfile { .. }),
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
        first.data_key_bytes(),
        second.data_key_bytes(),
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
const KEYFILE_LEN: usize = 77;

#[test]
fn the_keyfile_layout_is_what_every_existing_database_expects() {
    let keyfile = Keyring::create(PASSPHRASE).expect("create").to_keyfile();

    assert_eq!(
        keyfile.len(),
        KEYFILE_LEN,
        "the keyfile length changed, which means no database written by an earlier build opens",
    );
    assert_eq!(
        keyfile[VERSION_AT], 1,
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
        matches!(error, KeyringError::MalformedKeyfile { reason } if reason == "wrong length"),
        "a keyfile of the wrong length must say so, got: {error:?}",
    );
}

#[test]
fn a_keyring_does_not_print_its_key_material() {
    let keyring = Keyring::create(PASSPHRASE).expect("create");
    let rendered = format!("{keyring:?}");

    let key = keyring.data_key_bytes();
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
