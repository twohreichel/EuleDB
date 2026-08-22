//! The embedding pipeline: prefixes, chunking, normalisation, and the same answer twice.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative"
)]

use euledb_embed::{DIMENSIONS, Embedder, TOKEN_LIMIT};

/// The model directory, relative to the repository root.
fn model() -> Embedder {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the crate sits two levels below the repository root")
        .join("model");
    Embedder::load(&root).expect("the model is fetched — run `just model` if this fails")
}

#[test]
fn a_passage_embeds_to_the_documented_number_of_dimensions() {
    let embedder = model();
    let chunks = embedder
        .embed_passage("Grundsatzurteil zur Vorratsdatenspeicherung")
        .expect("a short passage embeds");

    assert_eq!(chunks.len(), 1, "a short passage is one chunk");
    assert_eq!(
        chunks[0].as_slice().len(),
        DIMENSIONS,
        "the model's hidden size is the vector's length, and the criterion names it",
    );
}

/// AC-33's hard half: the same input yields the same vector, bit for bit.
#[test]
fn the_same_text_embeds_bit_identically_twice() {
    let embedder = model();
    let text = "Rapport sur la souveraineté numérique européenne";

    let first = embedder.embed_passage(text).expect("embeds");
    let second = embedder.embed_passage(text).expect("embeds again");

    assert_eq!(
        first, second,
        "bit-identical, not merely close: a vector that drifts between runs makes an index wrong \
         rather than approximate",
    );
}

#[test]
fn every_vector_is_l2_normalised() {
    let embedder = model();
    let chunks = embedder
        .embed_passage("Ustawa o ochronie danych osobowych w Rzeczypospolitej Polskiej")
        .expect("embeds");

    for chunk in &chunks {
        let norm: f32 = chunk.as_slice().iter().map(|value| value * value).sum();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "an L2-normalised vector has length one, so cosine is a dot product; got {norm}",
        );
    }
}

/// The prefix convention is not decoration: E5 without it loses measurable recall.
///
/// So a test has to show the prefix reaches the model, not merely that a vector came back. Two calls on
/// the same text through the two entry points must differ — if they do not, the prefix is being dropped.
#[test]
fn the_query_and_passage_prefixes_reach_the_model() {
    let embedder = model();
    let text = "Vorratsdatenspeicherung";

    let as_passage = embedder.embed_passage(text).expect("embeds");
    let as_query = embedder.embed_query(text).expect("embeds");

    assert_ne!(
        as_passage[0], as_query,
        "the same text under the two prefixes must embed differently, or the prefix never arrived",
    );

    // And they must still be about the same thing: E5's prefixes shift the vector, they do not scatter it.
    let similarity: f32 = as_passage[0]
        .as_slice()
        .iter()
        .zip(as_query.as_slice())
        .map(|(a, b)| a * b)
        .sum();
    assert!(
        similarity > 0.8,
        "the two prefixed forms of one word must stay close, but cosine was {similarity}",
    );
}

/// Chunking is asserted without a forward pass, on purpose.
///
/// Embedding a 20 000-character document means one pass per chunk through a twelve-layer transformer,
/// and in a debug build that is minutes rather than seconds. The property under test is arithmetic —
/// no piece exceeds the window — so it is tested where it lives, in the tokenisation.
#[test]
fn text_beyond_the_token_limit_becomes_several_chunks_that_each_fit() {
    let embedder = model();
    let long = euledb_corpus::smoke()
        .into_iter()
        .max_by_key(|document| document.text.len())
        .expect("the corpus is not empty");
    assert!(
        long.text.len() > 20_000,
        "the fixture must actually be long, or this test proves nothing: {} chars",
        long.text.len(),
    );

    let chunks = embedder.chunks(&long.text).expect("a long passage chunks");
    assert!(
        chunks.len() > 1,
        "a document of {} characters cannot be one chunk of {TOKEN_LIMIT} tokens",
        long.text.len(),
    );

    for chunk in &chunks {
        let cost = embedder.token_count(chunk).expect("a chunk tokenises");
        assert!(
            cost <= TOKEN_LIMIT,
            "a chunk of {cost} tokens does not fit a window of {TOKEN_LIMIT}",
        );
    }

    // Nothing may be silently dropped: truncating loses the end of every long document, which is the
    // failure this chunking exists to avoid.
    let rejoined: usize = chunks.iter().map(String::len).sum();
    assert!(
        rejoined >= long.text.trim().len() - chunks.len(),
        "the chunks must account for the whole document: {rejoined} of {} characters",
        long.text.len(),
    );
}

/// A short text is one chunk and is not padded into several.
#[test]
fn a_short_text_is_one_chunk() {
    let embedder = model();
    let chunks = embedder
        .chunks("Grundsatzurteil zur Vorratsdatenspeicherung")
        .expect("chunks");
    assert_eq!(chunks.len(), 1, "a title is not several chunks");
}

#[test]
fn a_missing_model_says_how_to_fetch_it() {
    let empty = tempfile::tempdir().expect("a temporary directory is available");

    let failure = Embedder::load(empty.path()).expect_err("an absent model cannot be loaded");
    assert!(
        failure.to_string().contains("just model"),
        "the message must name the command that fixes it: {failure}",
    );
}

/// The embeddings must be *semantic*, which is the one property every other test in this file misses.
///
/// 384 dimensions, unit length, prefix-sensitivity and determinism all hold for a vector pooled the wrong
/// way — a mutation taking the leading token instead of the mean passes every one of them. What it cannot
/// pass is retrieval: E5 is trained with mean pooling, so a query must sit closer to the passage that
/// answers it than to one about something else entirely.
#[test]
fn a_query_is_closer_to_its_answer_than_to_an_unrelated_passage() {
    let embedder = model();

    let question = embedder
        .embed_query("Was regelt die Vorratsdatenspeicherung?")
        .expect("a query embeds");
    let answer = embedder
        .embed_passage(
            "Die Vorratsdatenspeicherung verpflichtet Anbieter, Verbindungsdaten ihrer Kunden für \
             eine bestimmte Frist zu speichern, damit Behörden später darauf zugreifen können.",
        )
        .expect("a passage embeds");
    let unrelated = embedder
        .embed_passage(
            "Als Flut wird das Steigen des Wasserstandes infolge der Gezeiten bezeichnet. Der \
             Zeitraum reicht von einem Niedrigwasser bis zum folgenden Hochwasser.",
        )
        .expect("a passage embeds");

    let cosine = |a: &euledb_embed::Embedding, b: &euledb_embed::Embedding| -> f32 {
        a.as_slice()
            .iter()
            .zip(b.as_slice())
            .map(|(x, y)| x * y)
            .sum()
    };
    let to_answer = cosine(&question, &answer[0]);
    let to_unrelated = cosine(&question, &unrelated[0]);

    assert!(
        to_answer > to_unrelated,
        "the answer must be nearer than the tides: {to_answer} against {to_unrelated}",
    );
    // A margin, not merely an ordering: two vectors pooled the wrong way can still order correctly by
    // accident, and a margin this size does not survive that.
    assert!(
        to_answer - to_unrelated > 0.15,
        "the margin must be real, but was only {}",
        to_answer - to_unrelated,
    );
}

/// Two unrelated short texts stay apart, which guards against a catastrophic pooling bug.
///
/// **What this does not do, measured rather than assumed:** it does not defend the padding mask. Pooling
/// the padded positions in was measured on this exact pair at cosine 0.8366 against 0.8165 with the mask
/// — real, and far too small for any honest threshold to separate. Tightening the bound to 0.83 would be
/// a number reverse-engineered from the mutation, which is the tautology this project bans.
///
/// The mask stays because pooling padding is simply wrong, not because a test here catches it. Testing it
/// properly needs one text embedded at two bucket sizes, and that means an API that exists only for the
/// test. Recorded as a gap instead.
#[test]
fn two_unrelated_short_texts_do_not_collapse_together() {
    let embedder = model();

    // Two short, unrelated texts. Both land in the smallest bucket, so both are mostly padding.
    let one = embedder.embed_query("Gezeiten").expect("embeds");
    let other = embedder.embed_query("Datenschutz").expect("embeds");

    let similarity: f32 = one
        .as_slice()
        .iter()
        .zip(other.as_slice())
        .map(|(a, b)| a * b)
        .sum();
    assert!(
        similarity < 0.9,
        "two unrelated short texts must not be near-identical, but cosine was {similarity} — which is \
         what pooling the padding in produces",
    );
}
