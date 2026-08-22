//! Signed capability tokens: what a handle is allowed to do, and proof that someone granted it.

use hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::Sha256;

use super::keyring::Keyring;

/// The context that separates a token key from every other key derived from the same secret.
///
/// Distinct from the data path's context on purpose: a token key and a data key must never be the same
/// bytes, or a token would be forgeable by anyone who can read a block header.
const CONTEXT: &[u8] = b"euledb/capability/v1";

/// How long a tag is. SHA-256, so 32 bytes, and the whole tag is compared — never a prefix.
const TAG_LEN: usize = 32;

/// What an operation is allowed to do.
///
/// Independent rather than a hierarchy: a write scope does **not** imply a read scope. Explicit is worth
/// the extra grant here, because an implicit escalation in an authorisation model is exactly the kind of
/// convenience that turns into a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Reading rows.
    Read,
    /// Adding, changing or removing rows.
    Write,
    /// Declaring or dropping a table, and building an index.
    Schema,
}

impl Scope {
    /// The scope as it goes into a signature. Stable: changing a byte here invalidates every token.
    const fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Read => b"read",
            Self::Write => b"write",
            Self::Schema => b"schema",
        }
    }

    /// The scope as it reads in a message.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Schema => "schema",
        }
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One granted permission, and the tag proving the authority granted it.
///
/// **What** — a table, a scope, and a signature over both. **Why signed** — a handle carries its
/// capabilities, so without a tag a holder could widen its own rights by editing the value. **Why
/// symmetric** — this database has one authority, the holder of the keyring, so an issuer that can also
/// verify costs nothing and avoids a key pair to manage.
///
/// The tag is not secret and the token is not a bearer credential for a network: it is only meaningful to
/// the keyring that signed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    table: String,
    scope: Scope,
    tag: [u8; TAG_LEN],
}

impl Capability {
    /// The table this permits something on.
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }

    /// What it permits.
    #[must_use]
    pub const fn scope(&self) -> Scope {
        self.scope
    }
}

impl Keyring {
    /// Grant a capability on one table.
    ///
    /// The keyring is the authority: it holds the secret the tag is derived from, so only a holder of the
    /// passphrase can widen what a handle may do.
    #[must_use]
    pub fn grant(&self, table: &str, scope: Scope) -> Capability {
        Capability {
            table: table.to_owned(),
            scope,
            tag: tag_for(self, table, scope),
        }
    }
}

/// The tag over one table and scope.
///
/// The table's length goes into the message before the table itself, so the message has exactly one
/// reading. **Not currently exploitable without it:** the scope vocabulary is three fixed words, and no
/// table name plus one of them concatenates to another table name plus another. The prefix costs eight
/// bytes and makes that argument unnecessary — if a scope ever becomes free-form, the encoding is
/// already canonical instead of needing to be revisited under time pressure.
fn tag_for(keyring: &Keyring, table: &str, scope: Scope) -> [u8; TAG_LEN] {
    let mut mac = Hmac::<Sha256>::new_from_slice(keyring.token_key().as_slice())
        .unwrap_or_else(|_| unreachable!("HMAC accepts a key of any length"));
    mac.update(CONTEXT);
    mac.update(&u64::try_from(table.len()).unwrap_or(u64::MAX).to_be_bytes());
    mac.update(table.as_bytes());
    mac.update(scope.as_bytes());
    mac.finalize().into_bytes().into()
}

/// The capabilities a handle carries, and the authority to check them against.
#[derive(Debug, Clone)]
pub(crate) struct Gate {
    key: [u8; TAG_LEN],
    granted: Vec<Capability>,
}

impl Gate {
    /// Build a gate from an authority and the set of permissions it granted.
    pub(crate) fn new(keyring: &Keyring, granted: Vec<Capability>) -> Self {
        Self {
            key: keyring.token_key(),
            granted,
        }
    }

    /// Whether this gate permits `scope` on `table`.
    ///
    /// A token is honoured only if its tag verifies under this gate's key, so a handed-over handle cannot
    /// widen itself by appending a capability it wrote by hand. Compared in constant time: a byte-wise
    /// comparison on a tag tells an attacker how much of a forgery was right.
    pub(crate) fn permits(&self, table: &str, scope: Scope) -> bool {
        self.granted.iter().any(|capability| {
            capability.scope == scope && capability.table == table && self.verifies(capability)
        })
    }

    /// Whether a token's tag was produced by this gate's authority.
    fn verifies(&self, capability: &Capability) -> bool {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.key.as_slice())
            .unwrap_or_else(|_| unreachable!("HMAC accepts a key of any length"));
        mac.update(CONTEXT);
        mac.update(
            &u64::try_from(capability.table.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        mac.update(capability.table.as_bytes());
        mac.update(capability.scope.as_bytes());
        mac.verify_slice(&capability.tag).is_ok()
    }
}
