//! Invariants of the repository itself.
//!
//! These are properties no compiler check expresses: that every crate root forbids `unsafe`, that
//! every manifest carries the metadata a registry requires, and that the pinned toolchain and the
//! declared minimum version have not drifted apart. They live in the suite because a convention only
//! survives if something fails when it is broken.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "in a test an unwrap IS the assertion, and its panic message is the failure narrative. \
              clippy.toml exempts #[cfg(test)] modules, but an integration test is its own crate, so \
              the exemption has to be stated here."
)]

use std::fs;
use std::path::{Path, PathBuf};

/// Absolute path of the workspace root, derived from this crate's own manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate sits at <root>/crates/<name>, so the root is two ancestors up")
        .to_path_buf()
}

/// Every member crate directory, discovered on the filesystem rather than listed here, so a crate
/// added tomorrow is covered by these invariants without anyone remembering to add it.
fn member_crate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(workspace_root().join("crates"))
        .expect("the workspace has a crates/ directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "no member crate found under crates/");
    dirs
}

fn read_toml(path: &Path) -> toml::Table {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
        .parse()
        .unwrap_or_else(|err| panic!("{} is not valid TOML: {err}", path.display()))
}

#[test]
fn every_crate_root_forbids_unsafe_code() {
    for dir in member_crate_dirs() {
        // A crate root is lib.rs or main.rs. Checking both means a binary crate added later is
        // covered rather than crashing this test with a confusing "cannot read lib.rs".
        let roots: Vec<PathBuf> = ["src/lib.rs", "src/main.rs"]
            .iter()
            .map(|name| dir.join(name))
            .filter(|path| path.is_file())
            .collect();
        // A deliberate tripwire rather than a silent skip: a crate whose root is somewhere else
        // (src/bin/*.rs, an explicit [[bin]] path) would otherwise go unchecked, and AC-1 admits no
        // unchecked crate root. Whoever adds one extends this list.
        assert!(
            !roots.is_empty(),
            "{} has a manifest but neither src/lib.rs nor src/main.rs, so its root would go unchecked",
            dir.display(),
        );
        for root in roots {
            let source = fs::read_to_string(&root)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", root.display()));
            assert!(
                source.contains("#![forbid(unsafe_code)]"),
                "{} does not declare #![forbid(unsafe_code)] at the crate root",
                root.display(),
            );
        }
    }
}

#[test]
fn every_member_manifest_states_the_registry_metadata() {
    // The fields crates.io rejects a publish without. Checked here as well as by
    // `cargo publish --dry-run`, because this runs offline in a second and names the missing key,
    // where the dry run needs the network and reports one crate at a time.
    const REQUIRED: [&str; 7] = [
        "description",
        "license",
        "repository",
        "keywords",
        "categories",
        "readme",
        "rust-version",
    ];

    for dir in member_crate_dirs() {
        let manifest_path = dir.join("Cargo.toml");
        let manifest = read_toml(&manifest_path);
        let package = manifest
            .get("package")
            .and_then(toml::Value::as_table)
            .unwrap_or_else(|| panic!("{} has no [package] table", manifest_path.display()));

        for key in REQUIRED {
            // Either stated outright or inherited with `key.workspace = true`. Whether an inherited
            // key actually resolves is cargo's business: it refuses to load the manifest at all when
            // the workspace does not supply the value, so checking it again here would be a branch
            // nothing can reach.
            assert!(
                package.contains_key(key),
                "{} states no `{key}`, so a publish would be rejected at the registry",
                manifest_path.display(),
            );
        }
    }
}

#[test]
fn the_pinned_toolchain_matches_the_declared_minimum_version() {
    let root = workspace_root();
    let pinned = read_toml(&root.join("rust-toolchain.toml"))
        .get("toolchain")
        .and_then(|t| t.get("channel"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .expect("rust-toolchain.toml pins [toolchain] channel");
    let declared = read_toml(&root.join("Cargo.toml"))
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("rust-version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .expect("[workspace.package] declares rust-version");

    assert_eq!(
        pinned, declared,
        "the pinned toolchain and the declared minimum version have drifted apart, so the suite \
         would no longer be verifying the version the crate claims to support",
    );
}
