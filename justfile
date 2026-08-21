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
    cargo publish --dry-run --workspace --all-features --allow-dirty

# Everything the gate covers, in the order a contributor should run it.
all: format lint test qa
