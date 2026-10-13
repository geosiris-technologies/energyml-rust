//! One Cargo feature per schema version: the table, the manifest and the
//! generated crates agree, and a version that is not compiled is refused by
//! name.
//!
//! These tests hold under **any** feature set - `--no-default-features
//! --features resqml20` as much as the default `all` - because what they read
//! is the always-compiled table, the manifest text and the directory listing.
//! The refusal is tested with the "compiled" set as a parameter, since a
//! workspace build unifies every feature on and could never reach the path
//! otherwise.

use std::collections::BTreeSet;
use std::path::PathBuf;

use energyml_schemas::{
    KNOWN_VERSIONS, KnownVersion, MissingFeature, known_version, known_version_of_content_type,
    known_version_of_qualified_type, missing_feature, refusal,
};

/// The manifest of this crate, as text.
const MANIFEST: &str = include_str!("../Cargo.toml");

fn generated_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../generated")
}

/// The `energyml-*` crate directories of `crates/generated/` (the ETP types,
/// `retp-types`, are no schema version).
fn crates_on_disk() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for entry in
        std::fs::read_dir(generated_dir()).expect("crates/generated/ is the home of the generated crates")
    {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("a utf-8 name")
            .to_string();
        if name.starts_with("energyml-") && path.join("Cargo.toml").is_file() {
            out.insert(name);
        }
    }
    out
}

/// The line of `[features]` that defines `feature`, e.g.
/// `resqml22 = ["dep:energyml-resqml-2-2", "eml23"]`.
fn feature_line(feature: &str) -> Option<String> {
    // Line endings are the checkout's business, not the test's.
    let manifest = MANIFEST.replace("\r\n", "\n");
    // On its own line: a comment above also mentions `[features]`.
    let features = manifest.split("\n[features]\n").nth(1)?.to_string();
    let features = features.split("\n[").next()?.to_string();
    features
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(&format!("{feature} = [")))
        .map(str::to_string)
}

/// Every generated crate has exactly one row and one feature name, and
/// conversely.
///
/// Both directions: a crate generated and given no row is a version the
/// refusal cannot name; a row with no crate is a feature that compiles
/// nothing. Neither would fail anything else.
#[test]
fn every_generated_crate_has_exactly_one_row_and_one_feature() {
    let on_disk = crates_on_disk();
    assert!(
        on_disk.len() >= 12,
        "the directory reader found {on_disk:?}; it is broken, not the table"
    );

    let rows: Vec<&str> = KNOWN_VERSIONS.iter().map(|row| row.crate_name).collect();
    let in_table: BTreeSet<String> = rows.iter().map(|name| name.to_string()).collect();
    assert_eq!(
        rows.len(),
        in_table.len(),
        "a crate has two rows in KNOWN_VERSIONS: {rows:?}"
    );
    assert_eq!(
        in_table, on_disk,
        "KNOWN_VERSIONS and crates/generated/ disagree: a new Energistics release costs one row there, one feature and one optional dependency in Cargo.toml"
    );

    let features: Vec<&str> = KNOWN_VERSIONS.iter().map(|row| row.feature).collect();
    let distinct: BTreeSet<&str> = features.iter().copied().collect();
    assert_eq!(
        features.len(),
        distinct.len(),
        "a feature name is used by two rows: {features:?}"
    );
}

/// The feature named by a row exists in `Cargo.toml`, switches on exactly its
/// crate, and brings the EML versions that crate is built on.
#[test]
fn every_row_names_a_feature_that_enables_its_crate_and_its_eml() {
    for row in KNOWN_VERSIONS {
        let line = feature_line(row.feature).unwrap_or_else(|| {
            panic!(
                "`{}` ({} {}) is named by KNOWN_VERSIONS and is no feature of Cargo.toml",
                row.feature, row.package, row.version
            )
        });
        assert!(
            line.contains(&format!("\"dep:{}\"", row.crate_name)),
            "`{line}` does not enable the crate `{}`",
            row.crate_name
        );
        for required in row.requires {
            assert!(
                line.contains(&format!("\"{required}\"")),
                "`{line}` does not enable `{required}`, which `{}` is built on",
                row.crate_name
            );
        }
        // Exactly those: the line holds the crate, the requirements, nothing else.
        let listed = line.matches('"').count() / 2;
        assert_eq!(
            listed,
            1 + row.requires.len(),
            "`{line}` enables more (or less) than `{}` and {:?}",
            row.crate_name,
            row.requires
        );

        // The crate is an optional dependency, never an unconditional one.
        let dependency = MANIFEST
            .lines()
            .find(|l| l.starts_with(&format!("{} = ", row.crate_name)))
            .unwrap_or_else(|| panic!("`{}` is not a dependency of Cargo.toml", row.crate_name));
        assert!(
            dependency.contains("optional = true"),
            "`{dependency}` is not optional"
        );
    }
}

/// What a row `requires` is what the generated crate's own manifest depends
/// on - read from the manifest, which is not code written here.
#[test]
fn the_requirements_are_the_eml_crates_the_generated_manifests_depend_on() {
    for row in KNOWN_VERSIONS {
        let path = generated_dir().join(row.crate_name).join("Cargo.toml");
        let manifest = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut depends_on: Vec<&str> = manifest
            .lines()
            .filter_map(|line| line.split_once(" = ").map(|(name, _)| name.trim()))
            .filter(|name| name.starts_with("energyml-eml-"))
            .filter_map(|name| {
                KNOWN_VERSIONS
                    .iter()
                    .find(|r| r.crate_name == name)
                    .map(|r| r.feature)
            })
            .collect();
        depends_on.sort_unstable();
        let mut required = row.requires.to_vec();
        required.sort_unstable();
        // An EML crate may depend on another EML crate; only the schema
        // families above EML are expected to name one.
        if row.package == "eml" {
            assert!(
                depends_on.is_empty() || depends_on == required,
                "{} depends on {depends_on:?}",
                row.crate_name
            );
        } else {
            assert_eq!(
                depends_on, required,
                "{} is built on {depends_on:?}, the table says {required:?}",
                row.crate_name
            );
        }
    }
}

/// The families and `all` hold every version, each exactly where it belongs.
#[test]
fn the_families_and_all_cover_every_version() {
    for row in KNOWN_VERSIONS {
        let family =
            feature_line(row.package).unwrap_or_else(|| panic!("no family feature `{}`", row.package));
        assert!(
            family.contains(&format!("\"{}\"", row.feature)),
            "family `{}` lacks `{}`",
            row.package,
            row.feature
        );
        for other in ["eml", "resqml", "witsml", "prodml"] {
            if other != row.package {
                assert!(
                    !feature_line(other)
                        .expect("a family")
                        .contains(&format!("\"{}\"", row.feature)),
                    "family `{other}` holds `{}`",
                    row.feature
                );
            }
        }
    }
    let all = feature_line("all").expect("`all`");
    for family in ["eml", "resqml", "witsml", "prodml"] {
        assert!(
            all.contains(&format!("\"{family}\"")),
            "`all` lacks the family `{family}`"
        );
    }
    assert!(
        MANIFEST.contains("default = [\"all\"]"),
        "the default of energyml-schemas is `all`: faces that never chose keep working"
    );
}

/// The table is not a second opinion that can drift from the build: the
/// features this build compiled are exactly the versions the registry holds.
#[test]
fn compiled_means_registered() {
    let registered = energyml_schemas::registered();
    let compiled = energyml_schemas::compiled_features();
    for row in KNOWN_VERSIONS {
        let in_registry = registered
            .iter()
            .any(|(package, version)| *package == row.package && *version == row.version);
        assert_eq!(
            row.is_compiled(),
            in_registry,
            "{} {}: feature `{}` is {} and the registry {} it",
            row.package,
            row.version,
            row.feature,
            if compiled.contains(&row.feature) {
                "on"
            } else {
                "off"
            },
            if in_registry { "holds" } else { "lacks" }
        );
    }
}

// ---------------------------------------------------------------------------
// The spellings of a version: one row each, whatever the document wrote
// ---------------------------------------------------------------------------

fn feature_of(row: Option<&'static KnownVersion>) -> &'static str {
    row.expect("a version this project ships").feature
}

/// `2.0`, `2.0.1`, `resqml20.X` and `resqml201.X` are one version - the 2.0.1
/// crate - and the `1` is not needed for a read. Held at the table, which is
/// what the refusal reads; the registry itself is held by `complete.rs`.
#[test]
fn every_spelling_of_resqml_2_0_1_is_the_resqml20_feature() {
    assert_eq!(feature_of(known_version("resqml", "2.0")), "resqml20");
    assert_eq!(feature_of(known_version("resqml", "2.0.1")), "resqml20");
    assert_eq!(feature_of(known_version("RESQML", " 2.0 ")), "resqml20");
    assert_eq!(
        feature_of(known_version_of_qualified_type("resqml20.obj_Activity")),
        "resqml20"
    );
    assert_eq!(
        feature_of(known_version_of_qualified_type("resqml201.obj_Activity")),
        "resqml20"
    );
    assert_eq!(
        feature_of(known_version_of_content_type(
            "application/x-resqml+xml;version=2.0;type=obj_Activity"
        )),
        "resqml20"
    );
    assert_eq!(
        feature_of(known_version_of_content_type(
            "application/x-resqml+xml;version=2.0.1;type=obj_Activity"
        )),
        "resqml20"
    );
}

/// The release and its development drop stay two rows: `2.2` is the release,
/// `2.2_dev3` and `2.2dev3` the drop, and a qualified type (which cannot name a
/// drop) is the release.
#[test]
fn a_release_and_its_drop_are_two_features() {
    assert_eq!(feature_of(known_version("resqml", "2.2")), "resqml22");
    assert_eq!(feature_of(known_version("resqml", "2.2_dev3")), "resqml22dev3");
    assert_eq!(feature_of(known_version("resqml", "2.2dev3")), "resqml22dev3");
    assert_eq!(
        feature_of(known_version_of_qualified_type(
            "resqml22.TriangulatedSetRepresentation"
        )),
        "resqml22"
    );
    assert_eq!(feature_of(known_version("eml", "2.3")), "eml23");
    assert_eq!(
        feature_of(known_version_of_qualified_type("eml23.Citation")),
        "eml23"
    );
}

/// A version nobody ships is not a missing feature: it stays unknown.
#[test]
fn a_version_nobody_ships_is_not_a_missing_feature() {
    for (package, version) in [
        ("resqml", "2.0.9"),
        ("resqml", "1.1"),
        ("witsml", "1.4.1.1"),
        ("energistics", "2.2"),
    ] {
        assert!(known_version(package, version).is_none(), "{package} {version}");
        assert!(missing_feature(package, version).is_none(), "{package} {version}");
    }
    assert!(known_version_of_qualified_type("resqml209.obj_Activity").is_none());
    assert!(known_version_of_qualified_type("witsml14.Well").is_none());
    assert!(known_version_of_qualified_type("not a qualified type").is_none());
    assert!(known_version_of_content_type("text/plain").is_none());
    assert!(refusal(None, &[]).is_none());
}

// ---------------------------------------------------------------------------
// The refusal, with the compiled set given
// ---------------------------------------------------------------------------

/// A version whose feature is off is refused by name; one whose feature is on
/// is not. The set is a parameter, so this runs with every feature on.
#[test]
fn a_version_that_is_not_compiled_is_a_named_refusal() {
    let row = known_version("resqml", "2.2");
    let refused = refusal(row, &["resqml20", "eml20"]).expect("resqml22 is not among the compiled features");
    assert_eq!(refused.feature(), "resqml22");
    assert_eq!(
        refused.to_string(),
        "RESQML 2.2 is not compiled in: enable the `resqml22` feature of energyml-schemas (or of energyml)"
    );
    // On, it is no refusal.
    assert!(refusal(row, &["resqml22", "eml23"]).is_none());
    // With nothing on, every row is refused, each by its own feature.
    for row in KNOWN_VERSIONS {
        let refused: MissingFeature = refusal(Some(row), &[]).expect("nothing is compiled");
        assert_eq!(refused.feature(), row.feature);
        assert!(refused.to_string().contains(&format!("`{}`", row.feature)));
        assert!(refused.to_string().starts_with(&row.package.to_ascii_uppercase()));
    }
    // The 2.0.1 spelling that only the release writes is refused as `resqml20`.
    let by_release =
        refusal(known_version_of_qualified_type("resqml201.obj_Activity"), &[]).expect("not compiled");
    assert_eq!(by_release.feature(), "resqml20");
    // The drop is its own refusal, and the release being on does not excuse it.
    let drop = refusal(known_version("resqml", "2.2_dev3"), &["resqml22", "eml23"])
        .expect("the drop is not compiled");
    assert_eq!(drop.feature(), "resqml22dev3");
    assert!(drop.to_string().contains("2.2_dev3"));
}

/// The same answer on the three lookup paths that can come back empty: the
/// pair a namespace and a `schemaVersion` give, the qualified type, and the
/// content type.
#[test]
fn the_refusal_is_the_same_on_every_lookup_path() {
    let none: &[&str] = &[];
    let by_pair = refusal(known_version("witsml", "2.1"), none);
    let by_qualified = refusal(known_version_of_qualified_type("witsml21.Well"), none);
    let by_content = refusal(
        known_version_of_content_type("application/x-witsml+xml;version=2.1;type=Well"),
        none,
    );
    assert_eq!(by_pair, by_qualified);
    assert_eq!(by_pair, by_content);
    assert_eq!(
        by_pair.expect("witsml21 is off in the empty set").feature(),
        "witsml21"
    );
}
