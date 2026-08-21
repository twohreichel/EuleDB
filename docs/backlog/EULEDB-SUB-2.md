---
id: EULEDB-SUB-2
ticket: EULEDB
fulfils: [AC-1, AC-65]
depends_on: [EULEDB-SUB-1]
size: M
context_budget: 3000
safety: additive, no public API yet
detail: stub
status: backlog
---

## Goal

Cargo workspace and local gate. Split the workspace into crates, forbid unsafe at every root, pin the MSRV in rust-toolchain.toml, and wire the justfile so format, lint, test and qa are one command each.

## Context (rough — regenerate this ticket just-in-time before starting it)

- `Cargo.toml (new)`
- `justfile (new)`
- `rust-toolchain.toml (new)`
- `docs/specs/spec.md (AC-1)`

## Claim the package name here, first thing after the metadata

Verified free on 2026-08-21 (crates.io, PyPI, npm — all 404; note `eule` on crates.io is TAKEN).
Neither registry reserves names: **the only way to claim one is to publish.** So once AC-65 metadata is
green under `cargo publish --dry-run`, publish a `0.0.0` placeholder to crates.io and PyPI before
anything else in this ticket. This needs the maintainer's own API tokens — an agent must not hold them,
so this step is executed by the maintainer or explicitly delegated.

Nothing else in the P0 chain is irreversible. This is.

## Two things that must not be guessed here

- **The MSRV is derived, not chosen.** It is the maximum `rust-version` across `lance`, `tantivy`,
  `arrow-rs` and `aes-gcm`. Read their manifests (`cargo add --dry-run`, or the `rust-version` field in
  the published crate) and pin that in `rust-toolchain.toml`. AC-11 verifies against it, so a guessed
  value makes the CI matrix meaningless.
- **Crate metadata is publish-blocking (AC-65).** `description`, `license = "Apache-2.0 OR MIT"`,
  `repository`, `keywords`, `categories`, `readme`, `rust-version`. crates.io rejects a publish without
  them, and AC-13 would discover that at the release tag — the worst moment. Prove it here with
  `cargo publish --dry-run`.
- **`UNSAFE.md` at the repository root**, not under `docs/` — that tree is ignored (AC-1).

## Not yet detailed

Steps, code sketch, verification commands and guardrails are written when this ticket becomes next.
A ticket detailed today against a repository state an earlier ticket will change is wrong by the time
it is picked up.
