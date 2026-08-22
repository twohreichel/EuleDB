//! What an update or a delete is told, and what it reports back.

/// A row filter, in the expression dialect the storage layer understands.
///
/// A newtype rather than a bare `&str`, for two reasons. It marks the value as something the storage
/// layer parses, so a caller cannot pass a user's text by accident and discover later that it was
/// evaluated. And it gives the eventual validated query representation somewhere to land: when the
/// query layer arrives, this is the type it produces, and every call site already takes it.
///
/// Nothing here validates the expression. The storage layer refuses one it cannot evaluate — an unknown
/// column is an error rather than a filter that matches nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate(String);

impl Predicate {
    /// A filter over a table's columns.
    ///
    /// ```
    /// use euledb_storage::Predicate;
    ///
    /// let recent = Predicate::new("published_at > 1700000000");
    /// assert_eq!(recent.as_str(), "published_at > 1700000000");
    /// ```
    #[must_use]
    pub fn new(expression: impl Into<String>) -> Self {
        Self(expression.into())
    }

    /// The expression as the storage layer receives it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Predicate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One column set to one expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    column: String,
    value: String,
}

impl Assignment {
    /// Set `column` to `value`, where `value` is an expression in the same dialect as a [`Predicate`].
    ///
    /// ```
    /// use euledb_storage::Assignment;
    ///
    /// // A literal string needs its quotes, because the right-hand side is an expression.
    /// let retitled = Assignment::new("title", "'Beschluss'");
    /// assert_eq!(retitled.column(), "title");
    /// ```
    #[must_use]
    pub fn new(column: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            value: value.into(),
        }
    }

    /// The column being set.
    #[must_use]
    pub fn column(&self) -> &str {
        &self.column
    }

    /// The expression it is set to.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// What an update did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Updated {
    /// How many rows changed.
    pub rows: u64,
}

/// What a delete did.
///
/// The count that was logged before the delete ran is deliberately **not** reported here as a second
/// number. At most one writer may hold a database, so nothing can change the table between the count and
/// the delete — the two can never differ, and a field that can never differ is surface without a claim.
/// The logged count is asserted where it belongs, against the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deleted {
    /// How many rows were removed.
    pub rows: u64,
}
