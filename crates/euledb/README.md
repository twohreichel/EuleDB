# euledb

Local-first embedded hybrid database. One encrypted file on disk that fuses three retrieval paths —
exact filters, vector semantics and BM25 full text — with no server, no daemon and no network call on
the query path.

**This crate is a placeholder at version 0.1.0.** The public API is empty on purpose: the storage
foundation beneath it is still being built, and an API exported before it can be honoured is a promise
rather than a feature. Watch the repository if you want to know when that changes.

What it is deliberately **not**: a server, a general-purpose SQL engine, a cloud sync service, or a
platform for training models. Analytical workloads belong in DuckDB.

- Repository, roadmap and specification: <https://github.com/twohreichel/EuleDB>
- Licence: `Apache-2.0 OR MIT`
