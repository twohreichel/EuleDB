# EuleDB task runner.
#
# These four commands are the quality gate CONTRIBUTING.md names, and CI runs the same ones, so a green
# local run and a green pipeline mean the same thing. `just` takes the toolchain from
# rust-toolchain.toml, which is the minimum supported version the pipeline also verifies against.
#
# Notes on choices that look odd until you know why:
#
# - `lint` denies warnings. A warning nobody has to fix is a warning everybody stops reading.
# - `test` invokes doctests separately, because nextest does not run them, and an example nobody
#   executes is a claim rather than an example.
# - `supply-chain` runs two tools because they cover different halves, not for redundancy. cargo-deny
#   evaluates normal and build dependencies and enforces the licence and source policy cargo-audit has
#   no concept of. cargo-audit reads Cargo.lock, so it also covers the dev tree — the crates that
#   execute during `cargo test` locally and in CI, which cargo-deny leaves out. See deny.toml.
# - `publish-check` passes --allow-dirty because this gate runs BEFORE the commit, and a dry run that
#   refuses to look at uncommitted work cannot check the work about to be committed. The real publish
#   runs from a clean tag checkout, where the flag is absent and the check is therefore stricter.
# `qa` prints three warnings that are expected, not findings: two "aborting upload due to dry run" from
# the publish check, and one about the repository-invariant test being excluded from the published
# package — it reads files above the package root, so it could not pass from a published crate.
#
# `lint-workflows` is deliberately NOT part of `lint`. It needs two binaries that have nothing to do
# with Rust, and failing a Rust contributor's gate with "command not found" would be worse than useless.
# The pipeline runs it on every change regardless, so the enforcement does not depend on remembering it.
#
# - No recipe uses shell-specific syntax. RUSTDOCFLAGS lives in .cargo/config.toml rather than being
#   prefixed onto a recipe, because `VAR=x cmd` is not valid on cmd.exe and Windows is supported.

# List the available recipes.
default:
    @just --list --unsorted

# Format every crate in place.
format:
    cargo fmt --all

# Reject anything the formatter or clippy objects to.
lint:
    cargo fmt --all --check
    cargo clippy --all-targets --all-features -- -D warnings

# Run the suite, including the examples in the documentation.
test:
    cargo nextest run --all-features
    cargo test --doc --all-features

# The gates that are not tests: documentation, publishability, supply chain.
qa: doc publish-check supply-chain

# Build the documentation, with a broken intra-doc link as a build failure.
doc:
    cargo doc --no-deps --all-features --workspace

# Check advisories, licences, banned crates and dependency sources.
supply-chain:
    cargo deny check
    cargo audit --deny warnings

# Prove every crate is publishable, rather than letting a release tag discover it is not.
publish-check:
    # The dry run serves the unpublished members to each other through a throwaway local registry, and
    # caches both the unpacked source and its compiled artefact under name and version. The version here
    # never changes, so cargo reuses the first of each forever and verifies a fresh facade against a stale
    # storage layer -- reporting a method as missing that exists. Drop both, and it rebuilds from the
    # tarball it just wrote. The compiled copy lands in the *workspace* target directory, not beside the
    # tarball, which is why the second line reaches there. Every other dependency stays cached.
    find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -mindepth 2 -maxdepth 2 -type d \
        -name 'euledb-*' -exec rm -rf {} +
    rm -rf target/*/.fingerprint/euledb-* target/*/deps/*euledb*
    cargo publish --dry-run --workspace --all-features --allow-dirty

# Fetch the reference corpus the benchmarks are measured against.
corpus:
    python3 scripts/fetch-corpus.py

# Fetch the embedding model at its pinned revision. Half a gigabyte, once.
model:
    python3 scripts/fetch-model.py

# Lint the GitHub Actions workflows and the Dependabot policy. Needs actionlint and zizmor on PATH.
lint-workflows:
    actionlint -color
    zizmor --persona=auditor .github/workflows/
    python3 scripts/check-dependabot.py

# Everything the gate covers, in the order a contributor should run it.
all: format lint test qa
