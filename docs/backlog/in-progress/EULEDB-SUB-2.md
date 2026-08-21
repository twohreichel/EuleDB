---
id: EULEDB-SUB-2
ticket: EULEDB
fulfils: [AC-1, AC-65]
depends_on: [EULEDB-SUB-1]
size: M
context_budget: 3000
safety: additive, no public API yet
detail: full
status: in-progress
---

## Goal

Cargo workspace and the local gate. Split the workspace into crates, forbid `unsafe` at every root,
pin the derived MSRV in `rust-toolchain.toml`, and wire the `justfile` so format, lint, test and qa are
one command each.

## Context (read ONLY these files)

- `Cargo.toml`, `rust-toolchain.toml`, `justfile`, `clippy.toml`, `.cargo/config.toml` (all new)
- `crates/euledb/`, `crates/euledb-storage/` (new)
- `UNSAFE.md` (new)
- `CONTRIBUTING.md` — it already promises the four commands
- `docs/specs/spec.md` (AC-1, AC-11, AC-65)

## The MSRV is derived, not chosen

`rust-version` of the dependencies the technology stack pins, read from the registry on 2026-08-21:

| Crate | Latest | `rust-version` |
|---|---|---|
| `lance` | 10.0.0 | **1.91.0** |
| `tantivy` | 0.26.1 | 1.86 |
| `arrow` | 59.2.0 | 1.85 |
| `aes-gcm` | 0.11.0 | 1.85 |
| `argon2` | 0.5.3 | 1.65 |
| `zstd` | 0.13.3 | 1.64 |

Maximum, and therefore the MSRV: **1.91.0**, pinned in `rust-toolchain.toml` and declared as
`rust-version` in `[workspace.package]`. A test asserts the two never drift apart, because AC-11
verifies against "the MSRV pinned in `rust-toolchain.toml`" and a stale pin makes that matrix leg
meaningless. This closes the § Open questions entry "The MSRV has no value yet".

## Crate split — the ladder verdict

Two crates, not three. `euledb` is the published facade (AC-23, AC-65) and `euledb-storage` is the
boundary AC-17 requires, where nothing outside may name a type from the on-disk format. A third
`euledb-core` was considered and rejected: AC-71 creates its first inhabitant in SUB-17, and an empty
crate added in advance is a placeholder, not a boundary.

The facade does **not** yet declare a dependency on the storage crate. It has no use for it, and an
unused dependency is dead weight — SUB-10 adds the edge when there is something behind it.

## Steps

1. Virtual workspace manifest with `resolver = "3"`, `members = ["crates/*"]`, and the AC-65 metadata
   in `[workspace.package]` so it is stated once instead of drifting per crate.
2. `rust-toolchain.toml` pinning 1.91.0 with `rustfmt` and `clippy`.
3. Both crates, each with `#![forbid(unsafe_code)]` at the root, a crate-level doc comment, and its own
   short `README.md` — cargo refuses a `readme` path outside the package root, and a symlink to the
   repository README breaks on a Windows checkout, which AC-11 claims as a supported platform.
4. `crates/euledb/tests/repository_invariants.rs` — the three invariants no compiler check expresses.
5. `justfile` with `format`, `lint`, `test`, `qa` plus the `doc` and `publish-check` recipes `qa`
   composes. No shell-specific syntax in any recipe: `RUSTDOCFLAGS` lives in `.cargo/config.toml`,
   because `VAR=x cmd` is not valid on `cmd.exe`.
6. `clippy.toml` plus `[workspace.lints.clippy]` denying `unwrap_used` and `expect_used`. Not required
   by this ticket's criteria — added deliberately, so that the habit AC-71 depends on is mechanical
   from the first crate rather than retrofitted across nine tickets.
7. `UNSAFE.md` at the repository root (AC-1), stating an empty inventory and the four conditions an
   exception has to satisfy. `CONTRIBUTING.md` points at it instead of describing it vaguely.
8. Verify on the MSRV toolchain AND on stable, which is the local half of AC-11.

## NOT done here: claiming the package name

`cargo publish --dry-run --workspace` passes, so AC-65 is met and the publish is unblocked. **The
publish itself is not performed.** It needs the maintainer's registry tokens, which an agent must not
hold, and it is the one irreversible step in the whole P0 chain — crates.io never deletes a version.

Note also a discrepancy to settle before publishing: this ticket's earlier draft called for a `0.0.0`
placeholder, while `.release-please-manifest.json` says `0.1.0`. The manifests say 0.1.0, because
Release Please owns the version from SUB-7 onwards and a hand-set 0.0.0 would fight it.

## Verification (executable)

```bash
just format && just lint && just test && just qa    # all four, in this order

# AC-11's two legs, locally: the pinned minimum AND stable
cargo --version                                      # 1.91.0, taken from rust-toolchain.toml
cargo +stable nextest run --all-features

# AC-1 holds by compiler enforcement, not by convention
grep -L '#!\[forbid(unsafe_code)\]' crates/*/src/lib.rs && echo "FAIL: a crate root is unguarded" \
  || echo "ok: every crate root forbids unsafe"

# the invariant tests must be able to FAIL — a test that only ever passes proves nothing
sed -i.bak '/^description = /d' crates/euledb-storage/Cargo.toml
cargo nextest run -E 'test(registry_metadata)'      # MUST fail, naming the crate and the key
mv crates/euledb-storage/Cargo.toml.bak crates/euledb-storage/Cargo.toml
```

## Out of scope / Guardrails

- **No `cargo publish`.** Dry run only — see above.
- **No `deny.toml`, no `cargo audit` in `qa`** — SUB-4 owns the supply-chain gates and adds them to
  `qa` then. A `cargo deny check` without its configuration is not a gate.
- **No workflow YAML** — SUB-3 to SUB-7.
- **No storage, schema or error types** — SUB-9 onwards. The crates stay empty on purpose.

## Definition of Done

- [ ] AC-1 covered: `#![forbid(unsafe_code)]` at every crate root, plus a tracked root `UNSAFE.md`
- [ ] AC-65 covered: full metadata, `cargo publish --dry-run --workspace` passing
- [ ] MSRV derived from the dependency manifests, pinned, and drift-guarded by a test
- [ ] All four `just` targets exist and pass, on the MSRV toolchain and on stable
- [ ] Every invariant test observed failing before it was made to pass
- [ ] Commits follow Conventional Commits, grouped by concern
