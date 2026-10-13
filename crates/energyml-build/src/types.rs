//! Choose a **type** from what the build knows — and say what a form is
//! allowed to change.
//!
//! The need: for feature types, the documentation gives the list of possible
//! objects. Display the list of objects that extend `AbstractObject` with a
//! default filter set to "feature", so as to show only the types that have
//! feature in their name, but the filter must be editable by the user. Operations
//! will sometimes impose packages and package versions. When the version and/or
//! the package is mandatory, the control is greyed out in the front end; otherwise
//! the input is enabled.
//!
//! # What this module holds, and what it does not
//!
//! It holds the **filtering**, which can be wrong: a form that offers a
//! type an operation cannot write is a form that makes a promise. It does not
//! hold the **list**: it comes from the schema registry, which the caller
//! owns — the crate's rule, *an operation reads nothing*, applied one step
//! earlier. `energyml-build` therefore does not depend on `energyml-schemas`, and one
//! test suffices to exercise the sorting with a few hand-written candidates.
//!
//! # Three states and not two
//!
//! *Any*, *this one by default* and *this one and nothing else* are
//! three different things, and confusing them costs on both sides: a default
//! presented as an obligation prevents a legitimate use, an obligation
//! presented as a default lets a package be written that the reader on the other side
//! will refuse. That is [`Constraint`], and it decides whether the front end greys the
//! control out or opens it.

use serde_json::{json, Value};

/// What a slot accepts of an axis — a package, a version — in three states.
///
/// Deliberately more general than the type choice alone: the same value
/// will serve to bound a `Need::Object` by package, or a free text field the
/// day an operation wants to. The rendering is the same on both sides —
/// a list or a field, greyed out or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    /// Everything the build knows. The control is open and empty.
    Any,
    /// These proposed, the others allowed. The control is open, pre-filled.
    Prefer(&'static [&'static str]),
    /// These and nothing else. The control is **greyed out**.
    Only(&'static [&'static str]),
}

impl Constraint {
    /// The named values, in the two cases where there are any.
    pub fn named(&self) -> &'static [&'static str] {
        match self {
            Constraint::Any => &[],
            Constraint::Prefer(list) | Constraint::Only(list) => list,
        }
    }

    /// Is the control fixed? — which decides the greying out.
    pub fn is_fixed(&self) -> bool {
        matches!(self, Constraint::Only(_))
    }

    /// Is this value allowed?
    ///
    /// `Any` and `Prefer` accept everything: the latter **proposes** without
    /// imposing, and that is the whole difference from the third.
    pub fn allows(&self, value: &str) -> bool {
        match self {
            Constraint::Any | Constraint::Prefer(_) => true,
            Constraint::Only(list) => list.iter().any(|one| one.eq_ignore_ascii_case(value)),
        }
    }

    /// What the control shows on opening — the first named value.
    pub fn suggested(&self) -> Option<&'static str> {
        self.named().first().copied()
    }

    /// The constraint, for a form: its values and whether to grey it out.
    pub fn json(&self) -> Value {
        json!({
            "kind": match self {
                Constraint::Any => "any",
                Constraint::Prefer(_) => "prefer",
                Constraint::Only(_) => "only",
            },
            "values": self.named(),
            "fixed": self.is_fixed(),
        })
    }
}

/// What a slot asks for when it asks for a **type**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeChoice {
    /// The proposed filter on the type name — `feature`, `interpretation`.
    ///
    /// **A default, never a limit**: the person clears or replaces it,
    /// and the list opens up. That is what was asked for, word for word.
    pub filter: &'static str,
    /// Of which packages — `resqml`, `witsml` — and whether the choice is fixed.
    pub package: Constraint,
    /// Of which versions — `2.2` — and whether the choice is fixed.
    pub version: Constraint,
    /// Offer only **concrete** types.
    ///
    /// An abstract type cannot be written: `AbstractFeature` is a name the
    /// schema gives to a family, not something a package can hold.
    /// Offering it would be offering a choice that fails on writing.
    pub concrete_only: bool,
}

impl TypeChoice {
    /// A type choice, filtered on this word, with no package constraint.
    pub fn filtered(filter: &'static str) -> Self {
        Self { filter, package: Constraint::Any, version: Constraint::Any, concrete_only: true }
    }

    /// The same, bound to this package and this version — fixed.
    pub fn only_in(mut self, package: &'static [&'static str], version: &'static [&'static str]) -> Self {
        self.package = Constraint::Only(package);
        self.version = Constraint::Only(version);
        self
    }

    /// The same, with this package and this version **proposed** and editable.
    pub fn prefer(mut self, package: &'static [&'static str], version: &'static [&'static str]) -> Self {
        self.package = Constraint::Prefer(package);
        self.version = Constraint::Prefer(version);
        self
    }

    /// What the form reads of it.
    pub fn json(&self) -> Value {
        json!({
            "filter": self.filter,
            "package": self.package.json(),
            "version": self.version.json(),
            "concreteOnly": self.concrete_only,
        })
    }
}

/// A type the build knows, as the caller describes it.
///
/// The strict minimum for sorting: `energyml-build` never sees a
/// `TypeInfo`, and that is what keeps it free of the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// `resqml22.BoundaryFeature`.
    pub qualified_type: String,
    /// `BoundaryFeature` — what the filter applies to.
    pub name: String,
    /// `resqml`, `witsml`.
    pub package: String,
    /// `2.2`.
    pub version: String,
    /// An abstract type cannot be written.
    pub is_abstract: bool,
}

/// The types a choice retains, in the order they arrived.
///
/// `filter` is what the person typed — empty, or absent, means *everything the
/// constraint lets through*, and not *nothing*. The comparison is
/// case-insensitive and applies to the **name**, not the qualified type: one
/// types `feature`, not `resqml22.`.
///
/// `package` and `version` are what the person chose when they are
/// allowed to; an `Only` constraint imposes them whatever is asked, because a
/// greyed-out list must return what it shows even if the call asks it for something
/// else.
pub fn matching<'a>(
    candidates: &'a [Candidate],
    choice: &TypeChoice,
    filter: Option<&str>,
    package: Option<&str>,
    version: Option<&str>,
) -> Vec<&'a Candidate> {
    let wanted = filter.unwrap_or(choice.filter).trim().to_ascii_lowercase();
    let axis = |constraint: &Constraint, asked: Option<&str>| -> Option<String> {
        match constraint {
            Constraint::Only(list) => list.first().map(|one| one.to_ascii_lowercase()),
            _ => asked
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_ascii_lowercase)
                .or_else(|| constraint.suggested().map(str::to_ascii_lowercase)),
        }
    };
    let in_package = axis(&choice.package, package);
    let in_version = axis(&choice.version, version);
    candidates
        .iter()
        .filter(|one| !(choice.concrete_only && one.is_abstract))
        .filter(|one| wanted.is_empty() || one.name.to_ascii_lowercase().contains(&wanted))
        .filter(|one| in_package.as_ref().is_none_or(|want| one.package.eq_ignore_ascii_case(want)))
        .filter(|one| in_version.as_ref().is_none_or(|want| one.version.eq_ignore_ascii_case(want)))
        .collect()
}

/// Is this type **writable** under this choice? — the refusal, named.
///
/// What `matching` hides, this refuses: a form sorts, a plan must
/// say *why* it will not write. Both read the same constraint, which is
/// the only way they have of staying in agreement.
pub fn refuses(choice: &TypeChoice, candidate: &Candidate) -> Option<String> {
    if choice.concrete_only && candidate.is_abstract {
        return Some(format!(
            "`{}` is abstract: the schema gives that name to a family, not to something a package can hold",
            candidate.qualified_type
        ));
    }
    if !choice.package.allows(&candidate.package) {
        return Some(format!(
            "`{}` is of package `{}`, and this operation writes {}",
            candidate.qualified_type,
            candidate.package,
            choice.package.named().join(" or ")
        ));
    }
    if !choice.version.allows(&candidate.version) {
        return Some(format!(
            "`{}` is version {}, and this operation writes {}",
            candidate.qualified_type,
            candidate.version,
            choice.version.named().join(" or ")
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(qualified: &str, name: &str, package: &str, version: &str, is_abstract: bool) -> Candidate {
        Candidate {
            qualified_type: qualified.into(),
            name: name.into(),
            package: package.into(),
            version: version.into(),
            is_abstract,
        }
    }

    fn build() -> Vec<Candidate> {
        vec![
            one("resqml22.BoundaryFeature", "BoundaryFeature", "resqml", "2.2", false),
            one("resqml22.CulturalFeature", "CulturalFeature", "resqml", "2.2", false),
            one("resqml22.AbstractFeature", "AbstractFeature", "resqml", "2.2", true),
            one("resqml22.HorizonInterpretation", "HorizonInterpretation", "resqml", "2.2", false),
            one("resqml20.obj_TectonicBoundaryFeature", "TectonicBoundaryFeature", "resqml", "2.0.1", false),
            one("witsml21.Well", "Well", "witsml", "2.1", false),
            one("witsml21.WellboreCompletion", "WellboreCompletion", "witsml", "2.1", false),
        ]
    }

    fn named(found: Vec<&Candidate>) -> Vec<String> {
        found.into_iter().map(|one| one.qualified_type.clone()).collect()
    }

    #[test]
    fn the_default_filter_shows_only_what_one_looks_for() {
        let choice = TypeChoice::filtered("feature").only_in(&["resqml"], &["2.2"]);
        assert_eq!(
            named(matching(&build(), &choice, None, None, None)),
            vec!["resqml22.BoundaryFeature", "resqml22.CulturalFeature"]
        );
    }

    #[test]
    fn an_abstract_type_is_never_offered() {
        // `AbstractFeature` carries the word and the version; it is not writable.
        // To make it fail: let `is_abstract` through.
        let choice = TypeChoice::filtered("feature").only_in(&["resqml"], &["2.2"]);
        assert!(!named(matching(&build(), &choice, None, None, None)).iter().any(|one| one.contains("Abstract")));
    }

    #[test]
    fn the_filter_can_be_changed_and_empty_opens_everything() {
        // The filter must be editable by the user — and clearing it
        // shows everything the constraint lets through, never nothing.
        let choice = TypeChoice::filtered("feature").only_in(&["resqml"], &["2.2"]);
        assert_eq!(
            named(matching(&build(), &choice, Some("interpretation"), None, None)),
            vec!["resqml22.HorizonInterpretation"]
        );
        assert_eq!(matching(&build(), &choice, Some(""), None, None).len(), 3);
    }

    #[test]
    fn an_imposed_package_cannot_be_asked_for_anything_else() {
        // The list is greyed out in the front end; here, it returns what it shows
        // even if the call asks for witsml. Two readers of one fact who
        // diverge is the shape of a bug once found here.
        let choice = TypeChoice::filtered("").only_in(&["resqml"], &["2.2"]);
        let found = named(matching(&build(), &choice, None, Some("witsml"), Some("2.1")));
        assert!(found.iter().all(|one| one.starts_with("resqml22.")), "{found:?}");
    }

    #[test]
    fn a_proposed_package_can_be_changed() {
        // Otherwise the input is enabled: `Prefer` opens the control, and
        // what is typed there wins.
        let choice = TypeChoice { filter: "", package: Constraint::Prefer(&["resqml"]), version: Constraint::Any, concrete_only: true };
        assert_eq!(
            named(matching(&build(), &choice, None, Some("witsml"), None)),
            vec!["witsml21.Well", "witsml21.WellboreCompletion"]
        );
        // And with nothing asked, the proposed value applies.
        assert!(named(matching(&build(), &choice, None, None, None)).iter().all(|one| one.starts_with("resqml")));
    }

    #[test]
    fn a_refusal_names_both_ends() {
        let choice = TypeChoice::filtered("feature").only_in(&["resqml"], &["2.2"]);
        let why = refuses(&choice, &one("witsml21.Well", "Well", "witsml", "2.1", false)).expect("refused");
        assert!(why.contains("witsml21.Well") && why.contains("resqml"), "{why}");
        let why = refuses(&choice, &one("resqml20.x", "x", "resqml", "2.0.1", false)).expect("refused");
        assert!(why.contains("2.0.1") && why.contains("2.2"), "{why}");
        assert_eq!(refuses(&choice, &one("resqml22.BoundaryFeature", "BoundaryFeature", "resqml", "2.2", false)), None);
    }

    #[test]
    fn the_three_states_are_not_confused() {
        assert!(!Constraint::Any.is_fixed());
        assert!(!Constraint::Prefer(&["resqml"]).is_fixed());
        assert!(Constraint::Only(&["resqml"]).is_fixed());
        // `Prefer` proposes and does not impose: that is what separates it from `Only`.
        assert!(Constraint::Prefer(&["resqml"]).allows("witsml"));
        assert!(!Constraint::Only(&["resqml"]).allows("witsml"));
    }
}
