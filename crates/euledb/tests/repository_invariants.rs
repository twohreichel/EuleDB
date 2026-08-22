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

        // Registry metadata is meaningless for a crate that is never published. Skipping it here is not
        // a loophole: `publish = false` is itself the statement, and it is checked rather than assumed.
        if package.get("publish").and_then(toml::Value::as_bool) == Some(false) {
            continue;
        }

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
fn documentation_that_ships_names_no_criterion_id() {
    // A `///` or `//!` comment reaches docs.rs, where "AC-70" is a reference the reader cannot follow.
    // The statement is what a consumer needs; the traceability belongs in the ticket and the commit.
    // Ordinary `//` comments are internal and may cite freely.
    let mut leaks = Vec::new();
    for dir in member_crate_dirs() {
        let mut sources = vec![dir.join("src")];
        while let Some(path) = sources.pop() {
            for entry in fs::read_dir(&path).into_iter().flatten().flatten() {
                let child = entry.path();
                if child.is_dir() {
                    sources.push(child);
                    continue;
                }
                if child.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                let Ok(source) = fs::read_to_string(&child) else {
                    continue;
                };
                for (number, line) in source.lines().enumerate() {
                    let trimmed = line.trim_start();
                    let ships = trimmed.starts_with("///") || trimmed.starts_with("//!");
                    if ships && line.contains("AC-") {
                        leaks.push(format!("{}:{}", child.display(), number + 1));
                    }
                }
            }
        }
    }
    assert!(
        leaks.is_empty(),
        "documentation that ships to docs.rs cites a criterion id the reader cannot resolve:\n  {}",
        leaks.join("\n  "),
    );
}

#[test]
fn every_member_is_either_publishable_or_says_it_is_not() {
    // The middle ground is the dangerous one: a crate with neither the metadata nor an explicit
    // `publish = false` would be discovered at a release tag, which is the worst moment.
    for dir in member_crate_dirs() {
        let manifest_path = dir.join("Cargo.toml");
        let manifest = read_toml(&manifest_path);
        let package = manifest
            .get("package")
            .and_then(toml::Value::as_table)
            .unwrap_or_else(|| panic!("{} has no [package] table", manifest_path.display()));

        let opted_out = package.get("publish").and_then(toml::Value::as_bool) == Some(false);
        let publishable = package.contains_key("description") && package.contains_key("readme");
        assert!(
            opted_out || publishable,
            "{} neither carries publish metadata nor declares `publish = false`",
            manifest_path.display(),
        );
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

/// The platforms this project claims to support, paired with the spelling the contribution guide uses.
///
/// Hand-written on purpose: it is the independent statement both the pipeline and the prose are
/// checked against, so drift on either side fails rather than one silently following the other.
const SUPPORTED_PLATFORMS: [(&str, &str); 4] = [
    ("linux-x86_64", "Linux x86_64"),
    ("linux-aarch64", "Linux aarch64"),
    ("macos-arm64", "macOS arm64"),
    ("windows-x86_64", "Windows x86_64"),
];

#[test]
fn the_test_matrix_covers_exactly_the_platforms_the_guide_claims() {
    let root = workspace_root();
    let workflow =
        fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("the CI workflow exists");
    let guide = fs::read_to_string(root.join("CONTRIBUTING.md")).expect("CONTRIBUTING.md exists");

    let mut in_matrix: Vec<&str> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- { name: "))
        .filter_map(|rest| rest.split(',').next())
        .collect();
    in_matrix.sort_unstable();

    let mut claimed: Vec<&str> = SUPPORTED_PLATFORMS.iter().map(|(id, _)| *id).collect();
    claimed.sort_unstable();

    assert_eq!(
        in_matrix, claimed,
        "the CI matrix and the supported-platform list disagree. A platform the matrix does not \
         cover must not be claimed as supported, and one it covers should not be a secret.",
    );

    for (id, prose) in SUPPORTED_PLATFORMS {
        assert!(
            guide.contains(prose),
            "the matrix runs {id} but CONTRIBUTING.md never mentions it as \"{prose}\"",
        );
    }
}

/// The crate allowed to know the on-disk format. Everything else must not name it.
const STORAGE_CRATE: &str = "euledb-storage";

/// The dependency the trait boundary exists to contain.
const ON_DISK_FORMAT: &str = "lance";

/// Whether a line names the format as a Rust path, a manifest key or an import.
///
/// Deliberately not a bare substring search: `balance` and `glance` are ordinary words, and a check
/// that fires on them would be turned off within a week.
fn names_the_format(line: &str) -> bool {
    [
        format!("{ON_DISK_FORMAT}::"),
        format!("use {ON_DISK_FORMAT}"),
        format!("{ON_DISK_FORMAT} ="),
        format!("{ON_DISK_FORMAT}.workspace"),
        format!("extern crate {ON_DISK_FORMAT}"),
    ]
    .iter()
    .any(|needle| line.contains(needle.as_str()))
}

#[test]
fn no_crate_outside_the_storage_layer_names_the_on_disk_format() {
    let mut leaks = Vec::new();

    for dir in member_crate_dirs() {
        if dir.file_name().and_then(|name| name.to_str()) == Some(STORAGE_CRATE) {
            continue;
        }
        // Sources and the manifest. A crate that does not declare the dependency cannot use it, so
        // checking the manifest catches the leak one step earlier than checking the code.
        let mut files = vec![dir.join("Cargo.toml")];
        let src = dir.join("src");
        if src.is_dir() {
            files.extend(
                fs::read_dir(&src)
                    .expect("a crate's src/ directory is readable")
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "rs")),
            );
        }
        for file in files {
            let Ok(source) = fs::read_to_string(&file) else {
                continue;
            };
            for (number, line) in source.lines().enumerate() {
                if names_the_format(line) {
                    leaks.push(format!(
                        "{}:{}: {}",
                        file.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        leaks.is_empty(),
        "the on-disk format is named outside `{STORAGE_CRATE}`, which makes a pinned, replaceable \
         dependency permanent:\n  {}",
        leaks.join("\n  "),
    );
}

/// The store type outside the storage layer: legitimate to use, never to publish.
///
/// A distinct check from the one above, because the rule is different. Naming the format's *crate*
/// outside the storage layer is always wrong. Naming the store *type* is not — the facade holds one.
/// What must not happen is re-exporting it, which would put the format's name in the published API
/// even though no line imports the format's crate.
#[test]
fn no_crate_outside_the_storage_layer_re_exports_the_store_type() {
    /// The type whose name carries the format.
    const STORE_TYPE: &str = "LanceStore";

    let mut leaks = Vec::new();

    for dir in member_crate_dirs() {
        if dir.file_name().and_then(|name| name.to_str()) == Some(STORAGE_CRATE) {
            continue;
        }
        let src = dir.join("src");
        if !src.is_dir() {
            continue;
        }
        let sources = fs::read_dir(&src)
            .expect("a crate's src/ directory is readable")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"));
        for file in sources {
            let Ok(source) = fs::read_to_string(&file) else {
                continue;
            };
            for (number, line) in source.lines().enumerate() {
                let code = line.trim();
                if code.starts_with("pub use") && code.contains(STORE_TYPE) {
                    leaks.push(format!("{}:{}: {code}", file.display(), number + 1));
                }
            }
        }
    }

    assert!(
        leaks.is_empty(),
        "`{STORE_TYPE}` is re-exported outside `{STORAGE_CRATE}`, which publishes the on-disk format \
         through the facade:\n  {}",
        leaks.join("\n  "),
    );
}
