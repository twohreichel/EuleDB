# Test fixture

`long-document.txt` is one Wikipedia article, 66714 characters, taken from the tracked subset of the
reference corpus described in `corpus/README.md`.

It lives here rather than being read from that corpus so this crate needs no dependency on it: a crate that
is published cannot depend on one that is not, and the chunking test needs exactly one long real document.

- Source: `wikimedia/wikipedia`, snapshot `20231101`, page id `fr-1786` — "Los Angeles"
- Licence: **CC BY-SA 4.0**, authored by Wikipedia contributors. Not this repository's `Apache-2.0 OR MIT`.
