//! *Which objects* - one composable answer, instead of one list per feature.
//!
//! # Why a selector and not a selection
//!
//! Everything an application does to several objects at once asks the same
//! question twice: *which ones*, then *what to do*. Written as a list of uuids,
//! the answer is dead the moment the package changes - it names what was there
//! when somebody clicked, not what is there now. Written as a **selector**, it
//! survives a reload, applies to objects that arrive later, can be shown back to
//! a person ("every `Grid2dRepresentation`"), written into a document and
//! replayed.
//!
//! That is the shape the viewer design argues for,
//! and it is deliberately **not** in the viewer: the same question is asked by
//! `energyml` on the command line, by the desktop UI and by the browser. A
//! binary that knows something the library does not is a bug in the layering.
//!
//! # What it costs to answer
//!
//! [`Selector::Type`] and [`Selector::Uuids`] are answered from the inventory -
//! no object is deserialized. [`Selector::Field`] has to read each candidate,
//! and says so by taking an [`ObjectSource`]. Composing them with [`Selector::AllOf`]
//! puts the cheap ones first, so a field match over a package of ten thousand
//! objects reads only what the other terms left standing.
//!
//! # What is not here yet
//!
//! §3.1 also wants selection by model, by `CollectionToDataObject`, by
//! `RepresentationSetRepresentation` and by the presence of a property of a
//! given kind. All four are **graph** questions rather than object questions,
//! and they want the edges rather than the objects - `energyml_storage::graph`
//! already builds those. They arrive as more variants of this enum, not as
//! another mechanism, which is the whole point of the shape.

use energyml_core::naming;
use energyml_core::value::Reflect;

use crate::metadata::value_at;
use crate::source::ObjectSource;

/// What a selector needs to know about an object without reading it.
///
/// Taken from the inventory rather than from the object: an application that
/// has listed a package already holds both of these, and asking it to
/// deserialize ten thousand objects to find the two hundred grids would make
/// the cheap selectors as expensive as the dear one.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    /// What identifies the object. A uuid here, a uri in a browser - the caller
    /// gets back exactly what it handed in.
    pub id: &'a str,
    /// The uuid used to read the object, when a selector has to.
    pub uuid: &'a str,
    /// `resqml22.Grid2dRepresentation`, or the bare type name.
    pub qualified_type: &'a str,
}

/// Which objects a rule is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    /// Everything the caller offered.
    All,
    /// By type, compared **loosely**: `Grid2dRepresentation`,
    /// `obj_Grid2dRepresentation` and `resqml22.Grid2dRepresentation` all name
    /// the same concept, and a person typing one of them means the others.
    Type(String),
    /// An explicit list, kept as the exception it is: a manual selection is
    /// still a selector, so the mechanism has one shape rather than two.
    Uuids(Vec<String>),
    /// A dotted path and the value it must hold - `Citation.Originator`,
    /// `Geosiris`. The path crosses references, as every path in this crate
    /// does, so `RepresentedInterpretation.Citation.Title` is one term.
    ///
    /// The comparison is **exact**, and that is a choice: a case-insensitive
    /// match would quietly join `Geosiris` and `GEOSIRIS`, which are two
    /// different strings in a file somebody will have to search later.
    Field {
        /// The dotted path, crossing references as `metadata::value_at` does.
        path: String,
        /// The value it must hold, compared exactly.
        equals: String,
    },
    /// Everything the inner selector does *not* name.
    Not(Box<Selector>),
    /// Every term must name it. Empty means everything, as an empty conjunction
    /// does - and that is worth knowing before writing one by accident.
    AllOf(Vec<Selector>),
    /// Any term naming it is enough. Empty means nothing.
    AnyOf(Vec<Selector>),
}

impl Selector {
    /// Whether answering this needs objects read, rather than the inventory.
    ///
    /// Not an optimisation detail: a caller in a browser uses it to decide
    /// whether it can answer without going back to the worker, and a caller
    /// over ETP to decide whether it is about to pull ten thousand objects
    /// across a socket.
    pub fn reads_objects(&self) -> bool {
        match self {
            Selector::All | Selector::Type(_) | Selector::Uuids(_) => false,
            Selector::Field { .. } => true,
            Selector::Not(inner) => inner.reads_objects(),
            Selector::AllOf(terms) | Selector::AnyOf(terms) => terms.iter().any(Selector::reads_objects),
        }
    }
}

/// Whether one type name means the same concept as another.
///
/// The qualified type carries a package prefix - `resqml22.Grid2dRepresentation` -
/// and the older releases spell the type itself `obj_Grid2dRepresentation`.
/// Both are stripped before comparing, which is what `naming::type_eq_loose`
/// does; the prefix is dropped here because it belongs to the *package*, not to
/// the type, and a selector that named it would stop matching the day a package
/// is translated to another release.
fn type_matches(qualified: &str, wanted: &str) -> bool {
    let local = qualified.rsplit('.').next().unwrap_or(qualified);
    let asked = wanted.rsplit('.').next().unwrap_or(wanted);
    naming::type_eq_loose(local, asked)
}

fn matches(selector: &Selector, candidate: &Candidate<'_>, source: &dyn ObjectSource) -> bool {
    match selector {
        Selector::All => true,
        Selector::Type(wanted) => type_matches(candidate.qualified_type, wanted),
        Selector::Uuids(list) => list.iter().any(|id| id == candidate.id || id == candidate.uuid),
        // Read by the id when the caller offered a uri - the version it names is
        // the version judged - and by the uuid otherwise.
        Selector::Field { path, equals } => match read_candidate(candidate, source) {
            // An object the source cannot hand over is **not** a match, and is
            // not an error either: a lazy store answers for what it has, and a
            // package with one unreadable part must not make every field
            // selector fail.
            None => false,
            Some(object) => {
                value_at(object.as_ref() as &dyn Reflect, path, source).as_deref() == Some(equals)
            }
        },
        Selector::Not(inner) => !matches(inner, candidate, source),
        Selector::AllOf(terms) => terms.iter().all(|term| matches(term, candidate, source)),
        Selector::AnyOf(terms) => terms.iter().any(|term| matches(term, candidate, source)),
    }
}

fn read_candidate(
    candidate: &Candidate<'_>,
    source: &dyn ObjectSource,
) -> Option<Box<dyn energyml_core::registry::DynObject>> {
    if energyml_core::identifiers::EtpUri::parse(candidate.id).is_ok() {
        source.object_at(candidate.id)
    } else {
        source.object(candidate.uuid)
    }
}

/// The candidates a selector names, in the order they were offered.
///
/// The order is the caller's and never the selector's: what a person sees is a
/// list they have already arranged, and a selection that reordered it would
/// move the rows under their cursor.
pub fn select<'a>(
    selector: &Selector,
    candidates: &[Candidate<'a>],
    source: &dyn ObjectSource,
) -> Vec<&'a str> {
    candidates
        .iter()
        .filter(|candidate| matches(selector, candidate, source))
        .map(|candidate| candidate.id)
        .collect()
}

/// How many of these a selector names, without collecting them.
pub fn count(selector: &Selector, candidates: &[Candidate<'_>], source: &dyn ObjectSource) -> usize {
    candidates
        .iter()
        .filter(|candidate| matches(selector, candidate, source))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::NoObjects;

    fn candidates() -> Vec<Candidate<'static>> {
        vec![
            Candidate {
                id: "a",
                uuid: "a",
                qualified_type: "resqml22.Grid2dRepresentation",
            },
            Candidate {
                id: "b",
                uuid: "b",
                qualified_type: "resqml20.obj_Grid2dRepresentation",
            },
            Candidate {
                id: "c",
                uuid: "c",
                qualified_type: "resqml22.PolylineSetRepresentation",
            },
        ]
    }

    #[test]
    fn everything_is_everything_the_caller_offered() {
        assert_eq!(select(&Selector::All, &candidates(), &NoObjects), ["a", "b", "c"]);
    }

    #[test]
    fn a_type_is_matched_across_releases_and_spellings() {
        // The same concept, spelled three ways by two releases. A selector that
        // matched the string would name one of the two grids and leave the
        // other out of every rule anybody writes.
        let found = select(
            &Selector::Type("Grid2dRepresentation".into()),
            &candidates(),
            &NoObjects,
        );
        assert_eq!(found, ["a", "b"]);
        let with_prefix = select(
            &Selector::Type("resqml20.obj_Grid2dRepresentation".into()),
            &candidates(),
            &NoObjects,
        );
        assert_eq!(with_prefix, ["a", "b"]);
    }

    #[test]
    fn a_list_is_a_selector_like_any_other() {
        let chosen = Selector::Uuids(vec!["c".into(), "gone".into()]);
        // Only what is there, and in the caller's order: a selection carried
        // over from another package names objects nobody can see.
        assert_eq!(select(&chosen, &candidates(), &NoObjects), ["c"]);
    }

    #[test]
    fn terms_compose_both_ways() {
        let grids_but_not_b = Selector::AllOf(vec![
            Selector::Type("Grid2dRepresentation".into()),
            Selector::Not(Box::new(Selector::Uuids(vec!["b".into()]))),
        ]);
        assert_eq!(select(&grids_but_not_b, &candidates(), &NoObjects), ["a"]);

        let either = Selector::AnyOf(vec![
            Selector::Uuids(vec!["a".into()]),
            Selector::Type("PolylineSetRepresentation".into()),
        ]);
        assert_eq!(select(&either, &candidates(), &NoObjects), ["a", "c"]);
    }

    #[test]
    fn an_empty_conjunction_is_everything_and_an_empty_disjunction_is_nothing() {
        // Worth pinning rather than discovering: a rule builder that starts
        // with no terms selects the whole package under one reading and none of
        // it under the other, and both are defensible until one is written down.
        assert_eq!(
            select(&Selector::AllOf(vec![]), &candidates(), &NoObjects).len(),
            3
        );
        assert_eq!(
            select(&Selector::AnyOf(vec![]), &candidates(), &NoObjects).len(),
            0
        );
    }

    #[test]
    fn a_field_term_says_it_will_read() {
        assert!(!Selector::Type("x".into()).reads_objects());
        assert!(
            Selector::AllOf(vec![
                Selector::Type("x".into()),
                Selector::Field {
                    path: "Citation.Originator".into(),
                    equals: "Geosiris".into()
                },
            ])
            .reads_objects()
        );
    }

    #[test]
    fn a_field_term_over_a_source_with_nothing_in_it_names_nobody() {
        // And does not fail: a store that cannot hand over an object answers
        // for what it has. Making this an error would let one unreadable part
        // take down every rule in the package.
        let term = Selector::Field {
            path: "Citation.Title".into(),
            equals: "x".into(),
        };
        assert!(select(&term, &candidates(), &NoObjects).is_empty());
    }
}
