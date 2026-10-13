//! Every version this project knows, compiled or not - and the refusal that
//! names the feature a missing one needs.
//!
//! The registry returned by [`all`](crate::all) holds what the Cargo features
//! compiled in. That is the point of the features, and it has one cost: ask it
//! for `resqml22.TriangulatedSetRepresentation` in a build without `resqml22`
//! and the answer is `None`, which looks exactly like a type nobody ever heard
//! of. This table is the other half. It is **not feature-gated**, so it can say
//! "that is RESQML 2.2, a version this project knows, and this build did not
//! compile it", and the message names the feature that would.
//!
//! A version this table does not know (`witsml14`, `resqml209`) is *not* a
//! missing feature: [`missing_feature`] answers `None` for it, and the caller's
//! own "unknown" stays what it was. Only a version we ship but did not compile
//! earns the named refusal.
//!
//! Nothing here changes how a version is resolved. `2.0`, `2.0.1`, `resqml20.X`
//! and `resqml201.X` all land on the one row of the 2.0.1 crate, and `2.2` is
//! the release while `2.2_dev3` is the drop (`naming::split_prerelease`).

use std::fmt;

use energyml_core::identity::{parse_content_type, parse_qualified_type};
use energyml_core::naming;

/// One schema version this project ships a generated crate for.
#[derive(Debug, PartialEq, Eq)]
pub struct KnownVersion {
    /// The package, lower case, as qualified types and content types write it:
    /// `eml`, `resqml`, `witsml`, `prodml`.
    pub package: &'static str,
    /// The version as the registry spells it (the directory name of the
    /// generated crate): `2.3`, `2.0.1`, `2.2_dev3`.
    pub version: &'static str,
    /// Every compact spelling a qualified type may carry for it, dots removed:
    /// RESQML 2.0.1 is `resqml20` (its `schemaVersion` is `2.0`) and fesapi
    /// also writes `resqml201`, so both read it.
    pub compact: &'static [&'static str],
    /// The Cargo feature of `energyml-schemas` (and of `energyml`) that compiles
    /// it in.
    pub feature: &'static str,
    /// The generated crate behind the feature.
    pub crate_name: &'static str,
    /// The features the generated crate is built on - the EML versions it
    /// depends on. Enabling `feature` enables these too.
    pub requires: &'static [&'static str],
}

/// Every version, **compiled in or not**. The one list the refusal reads, held
/// against `crates/generated/` and against `Cargo.toml` by `tests/features.rs`.
pub const KNOWN_VERSIONS: &[KnownVersion] = &[
    KnownVersion {
        package: "eml",
        version: "2.0",
        compact: &["20"],
        feature: "eml20",
        crate_name: "energyml-eml-2-0",
        requires: &[],
    },
    KnownVersion {
        package: "eml",
        version: "2.1",
        compact: &["21"],
        feature: "eml21",
        crate_name: "energyml-eml-2-1",
        requires: &[],
    },
    KnownVersion {
        package: "eml",
        version: "2.2",
        compact: &["22"],
        feature: "eml22",
        crate_name: "energyml-eml-2-2",
        requires: &[],
    },
    KnownVersion {
        package: "eml",
        version: "2.3",
        compact: &["23"],
        feature: "eml23",
        crate_name: "energyml-eml-2-3",
        requires: &[],
    },
    KnownVersion {
        package: "resqml",
        version: "2.0.1",
        compact: &["20", "201"],
        feature: "resqml20",
        crate_name: "energyml-resqml-2-0-1",
        requires: &["eml20"],
    },
    KnownVersion {
        package: "resqml",
        version: "2.2",
        compact: &["22"],
        feature: "resqml22",
        crate_name: "energyml-resqml-2-2",
        requires: &["eml23"],
    },
    KnownVersion {
        package: "resqml",
        version: "2.2_dev3",
        compact: &["22"],
        feature: "resqml22dev3",
        crate_name: "energyml-resqml-2-2-dev3",
        requires: &["eml22"],
    },
    KnownVersion {
        package: "witsml",
        version: "2.0",
        compact: &["20"],
        feature: "witsml20",
        crate_name: "energyml-witsml-2-0",
        requires: &["eml21"],
    },
    KnownVersion {
        package: "witsml",
        version: "2.1",
        compact: &["21"],
        feature: "witsml21",
        crate_name: "energyml-witsml-2-1",
        requires: &["eml23"],
    },
    KnownVersion {
        package: "prodml",
        version: "2.0",
        compact: &["20"],
        feature: "prodml20",
        crate_name: "energyml-prodml-2-0",
        requires: &["eml21"],
    },
    KnownVersion {
        package: "prodml",
        version: "2.2",
        compact: &["22"],
        feature: "prodml22",
        crate_name: "energyml-prodml-2-2",
        requires: &["eml23"],
    },
    KnownVersion {
        package: "prodml",
        version: "2.3",
        compact: &["23"],
        feature: "prodml23",
        crate_name: "energyml-prodml-2-3",
        requires: &["eml23"],
    },
];

impl KnownVersion {
    /// Whether the feature of this version is on in **this build**.
    pub fn is_compiled(&self) -> bool {
        self.is_compiled_in(&compiled_features())
    }

    /// Whether this version is among `compiled` - the same question with the
    /// set given, which is how a test asks it with every feature on.
    pub fn is_compiled_in(&self, compiled: &[&str]) -> bool {
        compiled.contains(&self.feature)
    }

    /// Whether this row is what `(package, version)` names, in any of the
    /// spellings the project reads: `2.0` and `2.0.1`, `20` and `201`, `2.2`
    /// for the release and `2.2_dev3` or `2.2dev3` for the drop.
    fn names(&self, package: &str, version: &str) -> bool {
        if !self.package.eq_ignore_ascii_case(package.trim()) {
            return false;
        }
        let (release, pre) = naming::split_prerelease(version);
        let wanted_pre = naming::split_prerelease(self.version).1;
        let same_drop = match (pre, wanted_pre) {
            (None, None) => true,
            (Some(a), Some(b)) => naming::prerelease_eq(a, b),
            _ => false,
        };
        same_drop && self.compact.contains(&naming::compact_version(release).as_str())
    }
}

/// The features of `energyml-schemas` that are on in this build, version
/// features only (never a family nor `all`).
pub fn compiled_features() -> Vec<&'static str> {
    let mut on = Vec::new();
    if cfg!(feature = "eml20") {
        on.push("eml20");
    }
    if cfg!(feature = "eml21") {
        on.push("eml21");
    }
    if cfg!(feature = "eml22") {
        on.push("eml22");
    }
    if cfg!(feature = "eml23") {
        on.push("eml23");
    }
    if cfg!(feature = "resqml20") {
        on.push("resqml20");
    }
    if cfg!(feature = "resqml22") {
        on.push("resqml22");
    }
    if cfg!(feature = "resqml22dev3") {
        on.push("resqml22dev3");
    }
    if cfg!(feature = "witsml20") {
        on.push("witsml20");
    }
    if cfg!(feature = "witsml21") {
        on.push("witsml21");
    }
    if cfg!(feature = "prodml20") {
        on.push("prodml20");
    }
    if cfg!(feature = "prodml22") {
        on.push("prodml22");
    }
    if cfg!(feature = "prodml23") {
        on.push("prodml23");
    }
    on
}

/// The row a `(package, version)` names - the pair a namespace and a
/// `schemaVersion` give, or the registry's own. `None` for a version this
/// project does not ship at all.
pub fn known_version(package: &str, version: &str) -> Option<&'static KnownVersion> {
    KNOWN_VERSIONS.iter().find(|known| known.names(package, version))
}

/// The row a qualified type names: `resqml22.Model`, `resqml20.obj_Activity`
/// and `resqml201.obj_Activity` (the first of the last two is the schema
/// version's spelling, the second the release's - one row). A qualified type
/// cannot name a development drop, so `resqml22.` is the release.
pub fn known_version_of_qualified_type(qualified_type: &str) -> Option<&'static KnownVersion> {
    let parts = parse_qualified_type(qualified_type).ok()?;
    KNOWN_VERSIONS.iter().find(|known| {
        known.package.eq_ignore_ascii_case(parts.domain)
            && naming::split_prerelease(known.version).1.is_none()
            && known.compact.contains(&parts.version)
    })
}

/// The row a content type names:
/// `application/x-resqml+xml;version=2.0.1;type=obj_Activity`.
pub fn known_version_of_content_type(content_type: &str) -> Option<&'static KnownVersion> {
    let parts = parse_content_type(content_type).ok()?;
    known_version(parts.domain, parts.version)
}

/// A version this project ships that this build did not compile - a **refusal,
/// not an absence**: the data may be perfectly good, and the build merely did
/// not ask for the crate that reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingFeature {
    /// The version that is not compiled.
    pub version: &'static KnownVersion,
}

impl MissingFeature {
    /// The Cargo feature that would bring it.
    pub fn feature(&self) -> &'static str {
        self.version.feature
    }
}

impl fmt::Display for MissingFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} is not compiled in: enable the `{}` feature of energyml-schemas (or of energyml)",
            self.version.package.to_ascii_uppercase(),
            self.version.version,
            self.version.feature
        )
    }
}

impl std::error::Error for MissingFeature {}

/// The refusal for a row, given the set of features that are on. `None` when
/// the row is among them (or there is no row - a version nobody ships is not a
/// missing feature).
///
/// Pure: the set is a parameter, so that a test can ask the question of every
/// feature being off in a build where they are all on.
pub fn refusal(known: Option<&'static KnownVersion>, compiled: &[&str]) -> Option<MissingFeature> {
    known
        .filter(|known| !known.is_compiled_in(compiled))
        .map(|version| MissingFeature { version })
}

/// Why `(package, version)` is not in the registry of **this build**, when the
/// reason is a feature that is off. Call it after a lookup came back empty:
/// `Some` turns "unknown type" into "RESQML 2.2 is not compiled in: enable the
/// `resqml22` feature…", `None` leaves the unknown what it was.
pub fn missing_feature(package: &str, version: &str) -> Option<MissingFeature> {
    refusal(known_version(package, version), &compiled_features())
}

/// [`missing_feature`] for a qualified type (`resqml22.Model`).
pub fn missing_feature_of_qualified_type(qualified_type: &str) -> Option<MissingFeature> {
    refusal(
        known_version_of_qualified_type(qualified_type),
        &compiled_features(),
    )
}

/// [`missing_feature`] for a content type
/// (`application/x-resqml+xml;version=2.2;type=…`).
pub fn missing_feature_of_content_type(content_type: &str) -> Option<MissingFeature> {
    refusal(known_version_of_content_type(content_type), &compiled_features())
}
