#![forbid(unsafe_code)]

//! Local-first embedded hybrid database.
//!
//! EuleDB fuses three retrieval paths — exact filters, vector semantics and BM25 full text — over one
//! encrypted file on disk, with no server and no network call on the query path.
//!
//! This crate is the public surface. It is deliberately empty until the storage foundation below it
//! exists: an API exported before it can be honoured is a promise, not a feature.
