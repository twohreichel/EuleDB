//! Row identity, and how many rows an operation had to look at.

/// The format's identity for one row of one table.
///
/// Stable for as long as the row exists, which is what makes it usable as the thing an index points at
/// and as a member of a set. It is **not** a key and carries no order a caller should read meaning into:
/// two rows written in key order may hold ids in any order at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowId(u64);

impl RowId {
    /// Wrap a raw identifier as it came from storage.
    ///
    /// Crate-internal: an identity is assigned by storage, never minted by a caller. It becomes public
    /// when some call accepts one, and not before.
    #[must_use]
    pub(crate) const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The identifier as storage spells it.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// How many rows an operation had to look at to produce its answer.
///
/// The number a claim about indexing has to be made against: wall-clock time measures the machine and
/// the weather, rows examined measures the plan. A lookup that examines every row of a table did not
/// use an index, however fast it happened to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RowsExamined(u64);

impl RowsExamined {
    /// The count.
    ///
    /// **Approximate above one thousand.** The engine renders this metric for human eyes, so 1 000 and
    /// 1 004 both arrive as `1.00 K` and cannot be told apart. That is enough for the only question
    /// asked of it — whether a plan examined a handful of rows or the whole table — and not enough for
    /// an exact equality assertion at scale. Assert an order of magnitude, never a precise count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A result, and what producing it cost in rows examined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measured<T> {
    /// The answer, identical to what the unmeasured call returns.
    pub value: T,
    /// How many rows the widest step of the plan had to look at.
    pub rows_examined: RowsExamined,
}

/// The widest `rows_scanned` any step of an executed plan reports.
///
/// The maximum rather than the sum, deliberately: a plan reads in stages, and a later stage re-reading
/// the handful of rows an earlier one selected has not examined them again in any sense a caller cares
/// about. What the question is really about is whether **some** step had to walk the whole table, and
/// that is the widest step.
///
/// Returns zero when the plan reports no such metric, which is honest rather than convenient: a missing
/// metric is not evidence of a narrow scan, and a zero fails an "examined the whole table" assertion
/// loudly instead of passing a "narrow" one quietly.
pub(crate) fn widest_scan(plan: &str) -> RowsExamined {
    const METRIC: &str = "rows_scanned=";

    let widest = plan
        .match_indices(METRIC)
        .filter_map(|(at, _)| {
            let rest = &plan[at + METRIC.len()..];
            let value = rest
                .find(|c: char| c != '.' && c != ' ' && !c.is_ascii_alphanumeric())
                .map_or(rest, |end| &rest[..end]);
            parse_human_count(value.trim())
        })
        .max()
        .unwrap_or(0);
    RowsExamined(widest)
}

/// Read back a count the engine formatted for a human: `1`, `1.00 K`, `2.50 M`.
///
/// Returns `None` for anything that is not one of those shapes, so a change in how the engine renders
/// its metrics surfaces as a failing measurement rather than as a silently wrong number.
fn parse_human_count(text: &str) -> Option<u64> {
    let (digits, scale) = match text.split_once(' ') {
        Some((digits, suffix)) => {
            let scale = match suffix {
                "K" => 1_000_u64,
                "M" => 1_000_000,
                "G" => 1_000_000_000,
                _ => return None,
            };
            (digits, scale)
        }
        None => (text, 1),
    };

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a rendered row count is non-negative and far below u64's range, and the value is \
                  documented as approximate above one thousand anyway"
    )]
    match digits.parse::<u64>() {
        Ok(exact) => Some(exact.saturating_mul(scale)),
        Err(_) => digits
            .parse::<f64>()
            .ok()
            .filter(|value| *value >= 0.0)
            .map(|value| (value * scale as f64) as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_human_count, widest_scan};

    /// A plan the engine actually produced, for a filter matching one row of a thousand.
    ///
    /// Kept verbatim rather than trimmed to the metric: the parsing has to survive the surrounding
    /// text, and a fixture reduced to what the parser wants would not test that.
    const REAL_PLAN: &str = "\
AnalyzeExec verbose=true, elapsed=4.292ms, metrics=[]
  TracedExec, elapsed=4.292ms, metrics=[]
    ProjectionExec: elapsed=4.292ms, expr=[id@0 as id, _rowid@1 as _rowid], metrics=[output_rows=1, elapsed_compute=14.88µs]
      LanceRead: elapsed=4.191ms, projection=[title], source=stream(_rowid), metrics=[output_rows=1, fragments_scanned=1, ranges_scanned=1, rows_scanned=1, bytes_read=8.13 K, iops=2]
        LanceRead: elapsed=2.794ms, projection=[id], row_id=true, full_filter=id = Int64(42), metrics=[output_rows=1, fragments_scanned=1, ranges_scanned=1, rows_scanned=1.00 K, bytes_read=4.71 K, iops=3]";

    #[test]
    fn the_widest_step_of_a_real_plan_is_the_full_table() {
        // Two steps report a scan: one row for the second read, a thousand for the filtered one. The
        // answer is the thousand — hand-read off the fixture above, not computed from it.
        assert_eq!(widest_scan(REAL_PLAN).get(), 1_000);
    }

    #[test]
    fn a_plan_without_the_metric_measures_zero() {
        assert_eq!(widest_scan("ProjectionExec: expr=[id@0 as id]").get(), 0);
    }

    #[test]
    fn a_rendered_count_reads_back_at_its_scale() {
        for (rendered, expected) in [
            ("0", Some(0)),
            ("1", Some(1)),
            ("999", Some(999)),
            ("1.00 K", Some(1_000)),
            ("2.50 M", Some(2_500_000)),
            ("1.00 G", Some(1_000_000_000)),
        ] {
            assert_eq!(
                parse_human_count(rendered),
                expected,
                "{rendered:?} must read back as {expected:?}",
            );
        }
    }

    #[test]
    fn an_unrecognised_rendering_is_refused_rather_than_guessed() {
        for refused in ["", "K", "abc", "1.00 T", "1.00 KiB", "-5"] {
            assert_eq!(
                parse_human_count(refused),
                None,
                "{refused:?} is not a count this parser understands and must not be guessed at",
            );
        }
    }
}
