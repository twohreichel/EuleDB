# EuleDB task runner. These four commands are the quality gate CONTRIBUTING.md names, and CI runs the
# same ones — so a green local run means the same thing as a green pipeline.

# List the available recipes.
default:
    @just --list --unsorted

# Format every crate in place.
format:
    cargo fmt --all

# Reject anything the formatter or clippy objects to. Warnings are errors: a warning nobody has to
# fix is a warning everybody stops reading.
lint:
    cargo fmt --all --check
    cargo clippy --all-targets --all-features -- -D warnings

# Run the suite. nextest does not execute doctests, so they get their own invocation — a documented
# example that is never run is a claim, not an example.
test:
    cargo nextest run --all-features
    cargo test --doc --all-features

# The gates that are not tests.
qa: doc publish-check

# Build the documentation. Warnings are denied via .cargo/config.toml, so a broken intra-doc link
# fails the build here and in CI alike.
doc:
    cargo doc --no-deps --all-features --workspace

# Prove every crate is publishable now, rather than letting a release tag discover it is not.
# --allow-dirty on purpose: this gate runs BEFORE the commit, and a dry run that refuses to look at
# uncommitted work cannot check the work you are about to commit. The real publish (AC-13) runs from a
# clean tag checkout, where the flag is absent and the check is therefore stricter, not weaker.
publish-check:
    cargo publish --dry-run --workspace --all-features --allow-dirty

# Everything the gate covers, in the order a contributor should run it.
all: format lint test qa
