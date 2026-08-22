<p align="center">
  <img src="assets/images/euledb-logo.svg" alt="EuleDB – Frag deine Daten" width="460">
</p>

# EuleDB

EuleDB is a local-first, embedded hybrid database written in Rust. It combines exact filters,
semantic vector search and BM25 full-text in a single file — and lets anyone query it in plain
language through a sandboxed, validated query layer. No cloud, no server, no SQL required.
Your data stays yours.

> Frag deine Daten – klug wie eine Eule, und alles bleibt bei dir. 🇪🇺

## Status

**Early development.** The storage foundation works: tables with a declared schema, insert, scan,
update, delete and drop, compressed, encrypted at rest with AES-256-GCM under a rotatable key, one
writer at a time and any number of readers. Nothing has been published to a registry yet.

**What the name promises is not built.** There is no vector search, no full-text search, no fusion of
the two, and no plain-language query path — those are the phases after this one. Do not choose EuleDB
today for what the first paragraph of this file describes. The cryptographic design has not been
audited.

## What it is for

- Searching your own data **by meaning**, not only by keyword, without any of it leaving your machine.
- Asking a question in plain language and **seeing what the system understood before it runs**, so you
  can trust an answer without learning a query language.
- Running on whatever hardware you have, from a workstation down to a low-power single-board computer.
- One encrypted file you can copy, back up and move. No daemon, no port, no account.

## What it deliberately is not

Knowing the boundaries is the fastest way to tell whether this is the right tool, so they are stated
rather than discovered:

- **Not a server.** No daemon, no network listener, no client/server protocol. It is a library you embed.
- **Not a general-purpose SQL engine.** No joins, no cross-table transactions, no query optimiser beyond
  the hybrid planner. Analytical workloads belong in DuckDB, and that is not a gap to be filled later.
- **Not a cloud service.** Multi-device convergence is peer-to-peer over a transport you supply. Nothing
  here calls home, and there is no hosted anything.
- **Not a place models are trained.** Models are consumed, never trained or fine-tuned.
- **Not a storage-format project.** The on-disk format is an existing one, pinned and kept behind a
  boundary. The interesting part is the query layer above it.

If you are unsure whether an idea fits inside those lines,
[open a discussion](https://github.com/twohreichel/EuleDB/discussions) before writing code. See
[CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Dual-licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT licence](LICENSE-MIT)

at your option. Copyright (c) 2026 Andreas Reichel.

Unless you state otherwise, any contribution you intentionally submit for inclusion in this work is
licensed under the same dual licence, without additional terms or conditions.
