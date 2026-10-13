//! Every schema version this workspace generates, in one registry.
//!
//! # Why this crate exists
//!
//! `energyml-storage` takes a [`Registry`] and never builds one. That is what
//! makes "a new schema version costs no code change in the storage layer" true
//! — the layer names no version, no crate and no package. Somebody still has to
//! *choose* which versions an application knows, and for the tools shipped here
//! the answer is "all of them".
//!
//! That choice used to be written out twice in production — once in the CLI,
//! once in the round-trip tool — and eighteen more times across the test
//! suites, each an independently maintained subset. Nothing detected an
//! omission: a new crate left out of one of those lists produced no compile
//! error and no failing test, only a tool that quietly could not read the new
//! version and a test suite that quietly never covered it.
//!
//! So the list lives here, once, and [`tests/complete.rs`] checks it against
//! `codegen.toml` — the file that declares which versions the workspace
//! produces. Add a release, forget the line, and a test fails naming it.
//!
//! # What a new Energistics release actually costs
//!
//! | | |
//! |---|---|
//! | `codegen.toml` | one `[[package]]` block, one `[fetch]` entry |
//! | this crate | one optional dependency, one feature, one `KNOWN_VERSIONS` row, one `register` line |
//! | everything else | **nothing** |
//!
//! Identification is entirely `TypeInfo`-driven: content types, qualified
//! types, part names and the version resolution in `EpcStore` are computed from
//! the registry rather than matched against a table. The geometry readers are
//! keyed on `TypeInfo::type_key` — the *concept* name, identical across
//! versions by construction — so a new release is read by the reader that
//! already exists.
//!
//! What does cost something is a release that **respells the insides** of a
//! type, and that has happened at every release so far. Those live in named
//! constants next to the code that reads them — `NODE_PATCH`, `SPLIT_FIELDS`,
//! `PARENT_GRID_FIELDS`, `INTERPRETS` — each a list of the spellings one
//! concept has worn. A new spelling is one string added to one list.
//!
//! # Choosing versions: one Cargo feature per schema version
//!
//! Each generated crate is an optional dependency, and a feature switches it
//! on. The features are spelled as Energistics spells a qualified type:
//!
//! | Feature | Compiles | Also enables |
//! |---|---|---|
//! | `eml20` `eml21` `eml22` `eml23` | EML 2.0 to 2.3 | |
//! | `resqml20` | RESQML 2.0.1 | `eml20` |
//! | `resqml22` | RESQML 2.2 | `eml23` |
//! | `resqml22dev3` | RESQML 2.2_dev3, the development drop | `eml22` |
//! | `witsml20` / `witsml21` | WITSML 2.0 / 2.1 | `eml21` / `eml23` |
//! | `prodml20` / `prodml22` / `prodml23` | PRODML 2.0 / 2.2 / 2.3 | `eml21` / `eml23` / `eml23` |
//! | `eml` `resqml` `witsml` `prodml` | the families: every version of one | |
//! | `all` | everything (the default) | |
//!
//! `default-features = false, features = ["resqml22"]` therefore gives RESQML
//! 2.2 and EML 2.3 and nothing else — no 2.0.1, no WITSML.
//!
//! **A version that is not compiled is a named refusal, never a half read.**
//! [`all`] registers only what is compiled; [`KNOWN_VERSIONS`] lists every
//! version the project ships, compiled or not, and [`missing_feature`] (with
//! its qualified-type and content-type forms) turns an empty lookup into
//! "RESQML 2.2 is not compiled in: enable the `resqml22` feature of
//! energyml-schemas (or of energyml)". The spelling of a version in a document
//! does not matter: `2.0`, `2.0.1`, `resqml20.X` and `resqml201.X` all name the
//! one `resqml20` feature.
//!
//! The refusal reaches the readers through the registry itself: [`all`] tells it,
//! with `Registry::declare_absent`, every shipped version it did not compile, so
//! the storage layer refuses a part of such a version by that message instead of
//! reading it as another release of the package (`tests/absent.rs`).
//!
//! # Everything is read; only releases are written
//!
//! [`all`] registers every version, a development drop included, because a
//! file written against the drop must still open. What a *new* object may be
//! made of is a narrower list, and [`may_create`] is where it is drawn — once,
//! so that every face offering a type to create offers the same ones.
//!
//! [`Registry`]: energyml_core::registry::Registry
//! [`tests/complete.rs`]: https://github.com/geosiris-technologies/energyml-rust

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod versions;

pub use versions::{
    KNOWN_VERSIONS, KnownVersion, MissingFeature, compiled_features, known_version,
    known_version_of_content_type, known_version_of_qualified_type, missing_feature,
    missing_feature_of_content_type, missing_feature_of_qualified_type, refusal,
};

use energyml_core::naming;
use energyml_core::reflect::TypeInfo;
use energyml_core::registry::Registry;

/// Every generated crate **compiled in**, registered.
///
/// Which crates those are is the business of the Cargo features of this crate
/// (one per schema version — see the crate documentation); with none on, the
/// registry is empty. A version that is not compiled is not silently absent:
/// [`missing_feature`] names the feature that would bring it.
#[allow(unused_mut)]
pub fn all() -> Registry {
    let mut registry = Registry::new();
    #[cfg(feature = "eml20")]
    energyml_eml_2_0::register(&mut registry);
    #[cfg(feature = "eml21")]
    energyml_eml_2_1::register(&mut registry);
    #[cfg(feature = "eml22")]
    energyml_eml_2_2::register(&mut registry);
    #[cfg(feature = "eml23")]
    energyml_eml_2_3::register(&mut registry);
    #[cfg(feature = "prodml20")]
    energyml_prodml_2_0::register(&mut registry);
    #[cfg(feature = "prodml22")]
    energyml_prodml_2_2::register(&mut registry);
    #[cfg(feature = "prodml23")]
    energyml_prodml_2_3::register(&mut registry);
    #[cfg(feature = "resqml20")]
    energyml_resqml_2_0_1::register(&mut registry);
    // Registered *before* the release it precedes. Order is deliberate and
    // `Registry::register` also refuses to let a drop displace a release, so
    // neither this line nor its position can re-point `resqml22.` at the drop.
    #[cfg(feature = "resqml22dev3")]
    energyml_resqml_2_2_dev3::register(&mut registry);
    #[cfg(feature = "resqml22")]
    energyml_resqml_2_2::register(&mut registry);
    #[cfg(feature = "witsml20")]
    energyml_witsml_2_0::register(&mut registry);
    #[cfg(feature = "witsml21")]
    energyml_witsml_2_1::register(&mut registry);
    declare_absent_versions(&mut registry, &compiled_features());
    registry
}

/// Tell `registry` which shipped versions it does not hold, each with the
/// refusal that names its feature.
///
/// What turns "a 2.0.1 part, in a build with RESQML 2.2 only" from a part read
/// as 2.2 (a plausible object with the fields the two releases share) into a
/// refusal that says what to enable. The set of compiled features is a
/// parameter so that a test can ask with a feature off in a build where all are
/// on; [`all`] passes the real one.
pub fn declare_absent_versions(registry: &mut Registry, compiled: &[&str]) {
    for known in KNOWN_VERSIONS {
        if let Some(missing) = refusal(Some(known), compiled) {
            registry.declare_absent(known.package, known.version, known.compact, missing.to_string());
        }
    }
}

/// Which `(package, version)` pairs [`all`] registers, from the registry
/// itself.
///
/// Derived rather than written down, so it cannot disagree with what `all`
/// actually did — which is the whole point of the test that consumes it.
pub fn registered() -> Vec<(&'static str, &'static str)> {
    let registry = all();
    let mut pairs: Vec<(&'static str, &'static str)> = registry
        .top_level_types()
        .map(|info| (info.package, info.version))
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// Whether a **new** object may be created as this type.
///
/// Two conditions, and a list offering types to create — the front end's *new
/// object* chooser, an operation's type field — must apply both:
///
/// - **it names a part.** Only a top-level type has a qualified type; a nested
///   one exists inside a field, and offering it would offer something that
///   cannot then be created;
/// - **its version is a release, never a development drop.** `resqml 2.2_dev3`
///   is registered so that the files written against it still open, and for
///   nothing else. No object is ever made in 2.2dev3: it is a version supported
///   only for those who built models before 2.2 was published, and almost
///   nobody supports it.
///
/// The second rule is not only a preference, and the two ways it bites are why
/// it is decided here rather than in each list:
///
/// - the drop declares the release's own `version="2.2"`, so most of its
///   top-level types carry **the very qualified type** of a released one.
///   Offered beside it, `resqml22.Model` was listed twice under one name — and
///   choosing either row made the *release's* type, because
///   [`Registry::create`] resolves by qualified type and a drop never owns a
///   key its release holds. A row that says one thing and does another;
/// - the rest exist **only** in the drop — `resqml22.CategoricalProperty`,
///   `resqml22.LocalDepth3dCrs`, `resqml22.MdDatum` — so there the key *is*
///   the drop's, and creating one writes an object no released 2.2 reader
///   knows.
///
/// Reading is untouched: the drop stays registered, addressable by its own
/// version through [`Registry::by_name`], and tried by the reader over ordered
/// candidates (ADR-0004, reading policy). Only the offer
/// changes.
///
/// Held by `tests/complete.rs`: `a_development_drop_is_read_but_never_offered`
/// and `every_creatable_type_is_what_its_qualified_type_creates`.
pub fn may_create(info: &TypeInfo) -> bool {
    info.qualified_type.is_some() && !naming::is_prerelease(info.version)
}
