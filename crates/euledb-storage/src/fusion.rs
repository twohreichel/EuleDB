//! Reciprocal Rank Fusion: one ranking out of two, and a record of where each hit came from.

use crate::measurement::RowId;

/// The default `k` of Reciprocal Rank Fusion.
///
/// Sixty is the value the original paper settled on and the one every comparable system uses. Its job is
/// to flatten the difference between adjacent ranks: with `k = 60` the gap between rank 1 and rank 2 is
/// small, so a document that both sources rank moderately well beats one that a single source ranks first.
pub const DEFAULT_K: u32 = 60;

/// The `k` used where the corpus is too small for the default to discriminate.
///
/// With sixty, the scores of rank 1 and rank 20 differ by a few percent — on a corpus of a few dozen
/// documents that is no ordering at all. A smaller `k` restores the difference between ranks, which is the
/// whole point of the formula.
pub const SMALL_CORPUS_K: u32 = 15;

/// Below this many documents, `k` drops to [`SMALL_CORPUS_K`].
pub const SMALL_CORPUS_THRESHOLD: usize = 100;

/// One row in a fused ranking, with the ranks it held in each source.
///
/// **Why the per-source ranks are part of the result** — a caller has to be able to see whether a hit came
/// from the semantic side, the lexical side or both. Without that, a fused score is an unexplainable
/// number, and an unexplainable ranking is one nobody can debug or trust.
#[derive(Debug, Clone, PartialEq)]
pub struct FusedHit {
    /// The row.
    pub row: RowId,
    /// The fused score. Higher is better.
    pub score: f32,
    /// Its rank on the semantic side, counting from one, if that side found it.
    pub vector_rank: Option<usize>,
    /// Its rank on the lexical side, counting from one, if that side found it.
    pub lexical_rank: Option<usize>,
}

/// A fused ranking, and the `k` that produced it.
///
/// The `k` is reported rather than assumed because it is not constant: a small corpus uses a smaller one,
/// and a caller comparing two result sets needs to know which was used.
#[derive(Debug, Clone, PartialEq)]
pub struct Fused {
    /// The ranking, best first.
    pub hits: Vec<FusedHit>,
    /// The `k` this ranking was computed with.
    pub effective_k: u32,
}

/// Fuse two ranked lists by Reciprocal Rank Fusion.
///
/// `score(d) = sum over sources of 1 / (k + rank(d))`, with ranks counting from one.
///
/// The `k` is chosen from the corpus size, not from the list lengths: two sources may each return ten
/// candidates from a million documents, and it is the million that decides whether ranks need
/// discriminating.
pub(crate) fn fuse(vector: &[RowId], lexical: &[RowId], documents: usize, limit: usize) -> Fused {
    let effective_k = if documents < SMALL_CORPUS_THRESHOLD {
        SMALL_CORPUS_K
    } else {
        DEFAULT_K
    };

    let mut hits: Vec<FusedHit> = Vec::new();
    let mut contribute = |list: &[RowId], lexical_side: bool| {
        for (position, row) in list.iter().enumerate() {
            let rank = position + 1;
            #[expect(
                clippy::cast_precision_loss,
                reason = "k plus a rank is far below the range where f32 loses whole numbers"
            )]
            let contribution = 1.0_f32 / (f64::from(effective_k) as f32 + rank as f32);

            match hits.iter_mut().find(|hit| hit.row == *row) {
                Some(hit) => {
                    hit.score += contribution;
                    if lexical_side {
                        hit.lexical_rank = Some(rank);
                    } else {
                        hit.vector_rank = Some(rank);
                    }
                }
                None => hits.push(FusedHit {
                    row: *row,
                    score: contribution,
                    vector_rank: (!lexical_side).then_some(rank),
                    lexical_rank: lexical_side.then_some(rank),
                }),
            }
        }
    };
    contribute(vector, false);
    contribute(lexical, true);

    // Descending by score, and by row id where scores tie — a ranking that reorders equal scores between
    // runs cannot be paginated.
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.row.get().cmp(&b.row.get()))
    });
    hits.truncate(limit);
    Fused { hits, effective_k }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_K, SMALL_CORPUS_K, fuse};
    use crate::measurement::RowId;

    /// Hand-computed from the formula, not from the code.
    ///
    /// Two sources, `k = 15` because the corpus is small. Row 7 is rank 1 on the vector side and rank 2 on
    /// the lexical side: `1/16 + 1/17 = 0.0625 + 0.058824 = 0.121324`. Row 3 is rank 1 on the lexical side
    /// only: `1/16 = 0.0625`. Row 9 is rank 2 on the vector side only: `1/17 = 0.058824`.
    #[test]
    fn the_scores_are_the_formula() {
        let vector = [RowId::new(7), RowId::new(9)];
        let lexical = [RowId::new(3), RowId::new(7)];

        let fused = fuse(&vector, &lexical, 40, 10);

        assert_eq!(fused.effective_k, SMALL_CORPUS_K);
        assert_eq!(
            fused.hits.len(),
            3,
            "three distinct rows across the two lists"
        );

        assert_eq!(fused.hits[0].row, RowId::new(7));
        assert!(
            (fused.hits[0].score - 0.121_324).abs() < 1e-5,
            "1/16 + 1/17 = 0.121324, got {}",
            fused.hits[0].score,
        );
        assert_eq!(fused.hits[1].row, RowId::new(3));
        assert!(
            (fused.hits[1].score - 0.0625).abs() < 1e-5,
            "1/16 = 0.0625, got {}",
            fused.hits[1].score,
        );
        assert_eq!(fused.hits[2].row, RowId::new(9));
        assert!(
            (fused.hits[2].score - 0.058_824).abs() < 1e-5,
            "1/17 = 0.058824, got {}",
            fused.hits[2].score,
        );
    }

    /// The assertion that catches a fusion which merely concatenates.
    ///
    /// Row 5 is rank 3 on both sides; row 1 is rank 1 on one side only. Two moderate placements must beat
    /// one good one — that is what the formula is *for*, and a concatenation would put row 1 first.
    #[test]
    fn a_row_both_sources_found_beats_one_either_found_alone() {
        let vector = [RowId::new(1), RowId::new(2), RowId::new(5)];
        let lexical = [RowId::new(3), RowId::new(4), RowId::new(5)];

        let fused = fuse(&vector, &lexical, 40, 10);

        // Hand-computed: row 5 is 1/18 + 1/18 = 0.111111, row 1 is 1/16 = 0.0625.
        assert_eq!(
            fused.hits[0].row,
            RowId::new(5),
            "a row both sides ranked third must outrank one a single side ranked first: {:?}",
            fused.hits,
        );
        assert!((fused.hits[0].score - 0.111_111).abs() < 1e-5);
    }

    #[test]
    fn the_ranks_say_which_side_found_each_row() {
        let vector = [RowId::new(7), RowId::new(9)];
        let lexical = [RowId::new(3), RowId::new(7)];

        let fused = fuse(&vector, &lexical, 40, 10);
        let both = &fused.hits[0];
        assert_eq!(
            both.vector_rank,
            Some(1),
            "row 7 was first on the vector side"
        );
        assert_eq!(both.lexical_rank, Some(2), "and second on the lexical side");

        let lexical_only = fused
            .hits
            .iter()
            .find(|hit| hit.row == RowId::new(3))
            .expect("row 3 is in the ranking");
        assert_eq!(
            lexical_only.vector_rank, None,
            "the vector side did not find it"
        );
        assert_eq!(
            lexical_only.lexical_rank,
            Some(1),
            "the lexical side ranked it first"
        );
    }

    #[test]
    fn a_large_corpus_uses_the_default_k() {
        let fused = fuse(&[RowId::new(1)], &[], 1_000, 10);
        assert_eq!(fused.effective_k, DEFAULT_K);
        // 1/61, hand-computed.
        assert!(
            (fused.hits[0].score - 0.016_393).abs() < 1e-5,
            "got {}",
            fused.hits[0].score
        );
    }

    /// A tie must break the same way every run, or a caller cannot page through the result.
    #[test]
    fn equal_scores_break_by_row_so_the_order_is_stable() {
        let vector = [RowId::new(9), RowId::new(4)];
        let lexical = [RowId::new(4), RowId::new(9)];

        let fused = fuse(&vector, &lexical, 40, 10);
        // Both rows hold ranks 1 and 2, so both score 1/16 + 1/17. The lower row id comes first.
        assert_eq!(
            fused
                .hits
                .iter()
                .map(|hit| hit.row.get())
                .collect::<Vec<u64>>(),
            vec![4, 9],
            "equal scores must order by row id: {:?}",
            fused.hits,
        );
    }

    #[test]
    fn the_limit_truncates_the_fused_ranking() {
        let vector = [RowId::new(1), RowId::new(2), RowId::new(3)];
        let lexical = [RowId::new(4), RowId::new(5)];

        let fused = fuse(&vector, &lexical, 40, 2);
        assert_eq!(fused.hits.len(), 2, "five distinct rows, two asked for");
    }
}
