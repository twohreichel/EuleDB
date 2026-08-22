//! The reference corpus is real, documented, and the same on every machine.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use std::collections::BTreeSet;

use euledb_corpus::{Document, smoke};

#[test]
fn the_vendored_subset_has_the_shape_its_documentation_claims() {
    let documents = smoke();

    // Hand-read off corpus/README.md, which is the artefact a third party reads before running the
    // benchmark. If the file and the document disagree, one of them is wrong and this says so.
    assert_eq!(
        documents.len(),
        39,
        "the vendored subset holds 39 documents, as its README records",
    );

    let languages: BTreeSet<&str> = documents.iter().map(|d| d.language.as_str()).collect();
    assert_eq!(
        languages.into_iter().collect::<Vec<&str>>(),
        vec!["de", "en", "fr", "pl"],
        "four languages, three of them morphologically different from each other",
    );

    let ids: BTreeSet<&str> = documents.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids.len(), documents.len(), "every document id is distinct");
}

#[test]
fn every_document_carries_real_text() {
    for document in smoke() {
        assert!(
            document.text.len() >= 500,
            "{}: a stub carries no signal for retrieval and the fetcher filters them out",
            document.id,
        );
        assert!(!document.title.is_empty(), "{}: no title", document.id);
        assert!(
            document.id.starts_with(&format!("{}-", document.language)),
            "{}: the id must name its language, so a mixed corpus stays traceable",
            document.id,
        );
    }
}

/// Tabs and newlines survive the round trip, because a document full of them would otherwise become
/// several documents.
#[test]
fn the_line_format_survives_text_that_contains_its_separators() {
    let documents = smoke();
    let with_newlines = documents
        .iter()
        .filter(|document| document.text.contains('\n'))
        .count();
    assert!(
        with_newlines > 0,
        "Wikipedia articles have paragraphs, so an escaping bug would show here — unless the fixture \
         lost them, which is itself the failure",
    );

    // And the loader must not have invented a record from an escaped separator.
    let ids: BTreeSet<&str> = documents.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids.len(), 39, "39 lines, 39 documents, no more");
}

#[test]
fn a_malformed_line_is_refused_rather_than_guessed_at() {
    // Too few fields AND too many. A fifth field means the escaping failed upstream, so the line is
    // damaged in a way that would otherwise pass silently and shift every document after it.
    for malformed in [
        "",
        "only-one-field",
        "id\tde",
        "id\tde\ttitle",
        "id\tde\ttitle\ttext\tand-a-fifth",
    ] {
        assert!(
            Document::from_line(malformed).is_none(),
            "{malformed:?} is not a document and must not be read as one",
        );
    }
}

#[test]
fn an_unfetched_corpus_says_how_to_fetch_it() {
    let empty = tempfile::tempdir().expect("a temporary directory is available");

    let failure = euledb_corpus::reference(empty.path())
        .expect_err("a corpus that was never fetched cannot be loaded");
    assert!(
        matches!(&failure, euledb_corpus::CorpusError::Missing { .. }),
        "the failure must be the absence itself: {failure:?}",
    );
    // A benchmark that fails with "no such file" and no instruction is a benchmark nobody runs.
    assert!(
        failure.to_string().contains("just corpus"),
        "the message must name the command that fixes it: {failure}",
    );
}

#[test]
fn a_corpus_that_is_not_the_pinned_one_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory is available");
    std::fs::create_dir(root.path().join("corpus")).expect("the directory is creatable");
    // A line that parses perfectly well and is simply not the corpus the numbers came from.
    std::fs::write(
        root.path().join("corpus/reference.tsv"),
        "de-1\tde\tTitel\tEin Text, der lang genug ist, um die Stub-Grenze zu überschreiten.\n",
    )
    .expect("the file is writable");

    let failure = euledb_corpus::reference(root.path())
        .expect_err("a different corpus must not be loaded as the reference one");
    assert!(
        matches!(&failure, euledb_corpus::CorpusError::Changed { .. }),
        "measuring against a different corpus is worse than not measuring: {failure:?}",
    );
    assert!(
        failure
            .to_string()
            .contains(euledb_corpus::REFERENCE_DIGEST),
        "the message must name the digest that was expected: {failure}",
    );
}
