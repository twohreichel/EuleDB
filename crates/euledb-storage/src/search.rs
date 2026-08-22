//! The seam between an exact filter and whatever generates candidates for a search clause.

use std::future::Future;

use crate::measurement::{RowId, RowIdSet};

/// Where the candidates for a search clause come from.
///
/// **What** — vector similarity or full-text ranking, neither of which exists yet. **Why a port with no
/// implementation** — the dependency has to point inwards: the query path decides that filtering happens
/// first, and a searcher plugged in later cannot change that by accident. A port at an I/O boundary is
/// justified at one implementation because it inverts the dependency arrow rather than abstracting over
/// variants.
///
/// **The contract, and it is the whole point:** an implementation considers only the rows in `within`. It
/// is handed a set that the exact filter has already narrowed, so it never ranks a row the caller has
/// excluded — and never spends work doing so.
pub trait CandidateSource {
    /// The best candidates for this source's clause, drawn only from `within`.
    ///
    /// # Errors
    ///
    /// Whatever the implementation's own failure is. The query path passes it through rather than
    /// interpreting it.
    fn candidates(
        &self,
        within: &RowIdSet,
        limit: usize,
    ) -> impl Future<Output = crate::Result<Vec<RowId>>> + Send;
}
