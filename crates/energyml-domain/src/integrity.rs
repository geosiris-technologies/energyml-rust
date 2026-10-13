//! The business rules that need **more than one object** to judge - the code
//! half of the rule set, beside the data half in [`crate::rules`].
//!
//! # Why code, when the rules are data
//!
//! [`crate::rules`] is the default and stays so: a rule the closed vocabulary
//! can say is written in JSON, never here. What lands here is what the
//! vocabulary cannot say, and each check below names the missing word:
//!
//! - **a group of objects**: *one definitive trajectory per wellbore* is a
//!   count over every trajectory of the package, not a property of one;
//! - **a membership kept elsewhere**: what a `DataobjectCollection` holds is
//!   written in a *third* object (`CollectionsToDataobjectsAssociationSet`),
//!   which no path from the collection reaches;
//! - **the values of an array**: *every value of a discrete property is a key
//!   of its table* reads two arrays, which the vocabulary never does
//!   (the rule vocabulary cannot reach array lengths or array contents);
//! - **a disjunction, or a set**: *IsListric is present iff the fault's only
//!   throw kind is normal* asks whether a list holds exactly one distinct
//!   value - `forEach` is a conjunction over the first element of each item.
//!
//! These are the anomalies found by the synthetic reference model audit
//! (ADR-0018). The contact-subject anomaly is a data rule
//! (`trg/contact-element-is-geological` in [`crate::rules::trg`]); the property
//! kind anomalies were already [`crate::property_kind::check`]'s.
//!
//! # The same findings, the same three outcomes
//!
//! Every check answers with a [`Finding`] of [`crate::rules`], so `check` files
//! them in one list and counts them one way: a violation weighs what its check
//! declares ([`Integrity::gravity`]), an uncheckable is neither - never a silent
//! pass. Every one was first a warning; the weights were then reviewed
//! against the `[ERROR]` model of Lutetia, and all
//! of them are errors but `house/time-series-asset-resolves`. None of the texts
//! cited below is an RFC 2119 *MUST* (the Energistics documents that use RFC
//! 2119 - the packaging conventions, ETP - write it in capitals; the reference
//! guides' business rules do not): the weight is that decision, not the
//! specification's.
//!
//! # House rules are named as such
//!
//! A `trg/` id cites a guide or a schema. A `house/` id is a reading this
//! repository chose where the specification is silent - a discrete value absent from its table, a series naming an unknown well - and
//! its documentation says so, and says where the decision is written.
//!
//! Nothing here opens a file: an [`ObjectSource`] lends the objects, an
//! [`ArraySource`] the arrays, and a check that needs arrays with none given
//! says so rather than passing.

use std::collections::BTreeMap;

use energyml_core::identity;
use energyml_core::value::{FieldValue, Reflect, Scalar};

use crate::property;
use crate::rules::{Finding, Gravity, Outcome, concepts_of};
use crate::source::{ArraySource, ObjectSource};
use crate::table::{Cell, Lookup};
use crate::values::Values;

/// One cross-object check: its id, what it means, and the code that judges.
pub struct Integrity {
    /// The id findings carry - `trg/…` for a rule of the standard, `house/…`
    /// for one of ours.
    pub id: &'static str,
    /// What the rule means, for the report.
    pub says: &'static str,
    /// How much a violation weighs - see [`Gravity`].
    pub gravity: Gravity,
    judge: fn(&Context<'_>, &Integrity, &mut Vec<Finding>),
}

/// What a check reads from: every object of the package, and where to fetch
/// what is not among them.
struct Context<'a> {
    objects: &'a [(&'a str, &'a dyn Reflect)],
    source: &'a dyn ObjectSource,
    arrays: Option<&'a dyn ArraySource>,
}

/// The shipped checks - the list `check` runs and counts.
///
/// A slice rather than a `match`, like every list here that will grow: a new
/// check is an entry and a function.
pub fn checks() -> &'static [Integrity] {
    CHECKS
}

const CHECKS: &[Integrity] = &[
    Integrity {
        id: "house/discrete-value-in-lookup",
        says: "every value of a property with a category lookup is a key of its table's key column (the reading is ours: common TRG 2.3 §9.1 says a KeyColumn makes the table a lookup table, not how a value names a row)",
        gravity: Gravity::Error,
        judge: discrete_value_in_lookup,
    },
    Integrity {
        id: "house/time-series-asset-resolves",
        says: "a time series' `asset identifier` names a well this package holds (the convention is ours: TimeSeriesData.xsd l. 67 makes the key text, not a reference; the URI grammar is the Energistics Identifier Specification v4.0 §6.3 p. 14)",
        gravity: Gravity::Warning,
        judge: time_series_asset_resolves,
    },
    Integrity {
        id: "trg/one-definitive-trajectory-per-wellbore",
        says: "a wellbore has at most one definitive trajectory (WITSML TRG 2.1 §16.6 p. 350, on Trajectory.Definitive: \"There can only be one trajectory per wellbore with definitive=true\")",
        gravity: Gravity::Error,
        judge: one_definitive_trajectory_per_wellbore,
    },
    Integrity {
        id: "trg/is-listric-iff-normal",
        says: "IsListric is present if and only if the fault is normal (RESQML Interpretations.xsd l. 659-662, TRG 2.2 §6.20 p. 188: \"Must be present if the fault is normal. Must not be present if the fault is not normal.\"; a fault is normal when normal is its only throw kind)",
        gravity: Gravity::Error,
        judge: is_listric_iff_normal,
    },
    Integrity {
        id: "trg/homogeneous-collection",
        says: "a collection declaring HomogeneousDatatype true holds data objects of one type only (EML 2.3 Collection.xsd l. 66-70: \"If true all data objects in the collection are of the same Energistics data type\")",
        gravity: Gravity::Error,
        judge: homogeneous_collection,
    },
    Integrity {
        id: "trg/homogeneous-association",
        says: "a collection association declaring HomogeneousDatatype true lists data objects of one type only (EML 2.3 Collection.xsd l. 82-86, the same sentence on SingleCollectionAssociation)",
        gravity: Gravity::Error,
        judge: homogeneous_association,
    },
    Integrity {
        id: "trg/wellbore-interpretation-set-holds-wellbores",
        says: "an earth model's WellboreInterpretationSet holds WellboreInterpretations only (RESQML TRG 2.2 §6.19 p. 187: \"the DataobjectCollection must only contain WellboreInterpretation\")",
        gravity: Gravity::Error,
        judge: wellbore_interpretation_set_holds_wellbores,
    },
];

/// Run every check over a package's objects.
///
/// `objects` is every object that read, with its uuid; `source` lends what a
/// reference names; `arrays`, when given, reads the arrays - without it, the
/// one check that needs values says *uncheckable* for each object it would
/// have judged.
pub fn run(
    objects: &[(&str, &dyn Reflect)],
    source: &dyn ObjectSource,
    arrays: Option<&dyn ArraySource>,
) -> Vec<Finding> {
    let context = Context {
        objects,
        source,
        arrays,
    };
    let mut out = Vec::new();
    for check in CHECKS {
        (check.judge)(&context, check, &mut out);
    }
    out
}

// ---------------------------------------------------------------------------
// A discrete value absent from its table
// ---------------------------------------------------------------------------

/// **House rule**, and the reading is written down: the value is looked up in the
/// key column - because neither
/// the common TRG 2.3 §9.1-9.2 (p. 190-191) nor the RESQML TRG 2.2 §7.7
/// (p. 230) says how a value designates a row, beyond *"Defines a key column
/// which make the table a lookup table"*. `energyml-domain::table` and the front end
/// read it the same way; a value no key matches is a value no legend can name.
///
/// A value equal to the array's own `NullValue` is not a value, and `NaN` is
/// how floating arrays say the same thing.
fn discrete_value_in_lookup(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    'objects: for (uuid, object) in context.objects {
        if !concepts_of(*object).contains(&"AbstractProperty") {
            continue;
        }
        let Ok(read) = property::read(*object) else {
            continue;
        };
        let Some(table_uuid) = read.lookup.as_deref() else {
            continue;
        };
        let cant = |why: String| unverifiable(check, uuid, why);
        let Some(arrays) = context.arrays else {
            out.push(cant(
                "its values are an array, and this check was given nothing to read arrays with".into(),
            ));
            continue;
        };
        // By the uri the property names: the version it names is the table read.
        let found = match read.lookup_uri.as_deref() {
            Some(uri) => context.source.object_at(uri),
            None => context.source.object(table_uuid),
        };
        let Some(table) = found else {
            out.push(cant(format!("its lookup `{table_uuid}` is not in this package")));
            continue;
        };
        let lookup = match Lookup::read(table.as_reflect(), arrays) {
            Ok(lookup) => lookup,
            Err(why) => {
                out.push(cant(format!("its lookup `{table_uuid}` does not read: {why}")));
                continue;
            }
        };
        let [key] = lookup.key_columns.as_slice() else {
            out.push(cant(format!(
                "its lookup `{table_uuid}` has {} key column(s), and a value can name a row by one",
                lookup.key_columns.len()
            )));
            continue;
        };
        match lookup.rows.columns.get(*key) {
            Some(Some(Values::Text(_))) => {
                out.push(cant(format!(
                    "the key column of `{table_uuid}` holds text, and the property's values are numbers"
                )));
                continue;
            }
            Some(Some(_)) => {}
            _ => {
                out.push(cant(format!(
                    "the key column of `{table_uuid}` does not read: {}",
                    lookup.notes.join("; ")
                )));
                continue;
            }
        }
        let key_title = lookup.titles.get(*key).cloned().flatten();

        // Value → how many elements carry it, in order of first appearance.
        let mut missing: Vec<(f64, usize)> = Vec::new();
        for patch in &read.patches {
            let values = match patch.read(arrays) {
                Ok(values) => values,
                Err(why) => {
                    out.push(cant(format!("patch {} does not read: {why}", patch.position)));
                    continue 'objects;
                }
            };
            let Some(numbers) = values.to_f64() else {
                out.push(cant(format!("patch {} does not hold numbers", patch.position)));
                continue 'objects;
            };
            let null = number(patch.array, "NullValue");
            for value in numbers {
                if value.is_nan() || Some(value) == null {
                    continue;
                }
                if !lookup.find(&[Cell::Number(value)]).is_empty() {
                    continue;
                }
                match missing.iter_mut().find(|(known, _)| *known == value) {
                    Some((_, count)) => *count += 1,
                    None => missing.push((value, 1)),
                }
            }
        }
        if !missing.is_empty() {
            let listed: Vec<String> = missing
                .iter()
                .map(|(value, count)| format!("{value} ({count} element(s))"))
                .collect();
            out.push(violation(
                check,
                uuid,
                format!(
                    "{} not in the key column{} of `{table_uuid}`",
                    listed.join(", "),
                    key_title.map(|title| format!(" `{title}`")).unwrap_or_default()
                ),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// A series naming a well the package does not hold
// ---------------------------------------------------------------------------

/// **House rule.** A PRODML `Key` is
/// text - TimeSeriesData.xsd l. 67 makes it no reference - so nothing in the
/// schema ties a series to a well; the house convention does, with an
/// Energistics URI (Identifier Specification v4.0 §6.3 p. 14: `path-part =
/// component-type [ "(" identifier ")" ]`, and §6.3.1: *"The actual component
/// name or ID of the component is contained within parenthesis"*).
///
/// So the `well(…)` part of an `asset identifier` is looked for among the
/// package's WITSML wells, by the **name** WITSML 2 gives a well: its
/// `Citation.Title`, or its `NameLegal`. A name is not an identity - this
/// repository's rule - so two wells answering to
/// the name make the series **uncheckable**, never matched to the first.
///
/// What is left alone, and why: a URI with no `well(…)` part names something
/// else (a facility, a tag) and is not this rule's business; the `wellbore(…)`
/// part is not resolved, the convention stating no field its identifier
/// matches.
fn time_series_asset_resolves(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    let wells: Vec<(&str, Vec<String>)> = context
        .objects
        .iter()
        .filter(|(_, object)| object.type_info().type_key == "Well")
        .map(|(uuid, object)| (*uuid, names_of(*object)))
        .collect();

    for (uuid, object) in context.objects {
        if !matches!(
            object.type_info().type_key,
            "TimeSeriesData" | "TimeSeriesStatistic"
        ) {
            continue;
        }
        for key in objects(*object, "Key") {
            if text(key, "keyword").as_deref() != Some("asset identifier") {
                continue;
            }
            let Some(uri) = text(key, "value") else {
                continue;
            };
            let name = match well_named_by(&uri) {
                Ok(Some(name)) => name,
                Ok(None) => continue,
                Err(why) => {
                    out.push(unverifiable(check, uuid, format!("`{uri}`: {why}")));
                    continue;
                }
            };
            let found: Vec<&str> = wells
                .iter()
                .filter(|(_, names)| names.contains(&name))
                .map(|(well, _)| *well)
                .collect();
            match found.as_slice() {
                [_] => {}
                [] => out.push(violation(
                    check,
                    uuid,
                    format!(
                        "`{uri}` names the well `{name}`, and no well of this package is named so ({} well(s) read, by Title and NameLegal)",
                        wells.len()
                    ),
                )),
                several => out.push(unverifiable(
                    check,
                    uuid,
                    format!(
                        "`{uri}` names the well `{name}`, and {} wells answer to that name ({}): a name is not an identity",
                        several.len(),
                        several.join(", ")
                    ),
                )),
            }
        }
    }
}

/// The names WITSML 2 gives a well: its `Citation.Title`, and its
/// `NameLegal` when it has one. Shared with `series::time_series`, which
/// matches a series to a well by the same names - one reading of *which well
/// this name means*, not two.
pub(crate) fn names_of(object: &dyn Reflect) -> Vec<String> {
    let mut names: Vec<String> = identity::title_of_reflect(object)
        .map(str::to_owned)
        .into_iter()
        .collect();
    names.extend(text(object, "NameLegal"));
    names
}

/// The identifier of the `well(…)` part of an Energistics URI - `None` when
/// the path has no such part, an error when the text is not such a URI.
///
/// Energistics Identifier Specification v4.0 §6.1 p. 13: `emlUri = "eml:" [
/// "//" authority ] "/" path` - the scheme is not case sensitive, the path is
/// (§6.3 p. 14).
pub(crate) fn well_named_by(uri: &str) -> Result<Option<String>, String> {
    let rest = uri.trim();
    let rest = match rest.get(..4) {
        Some(scheme) if scheme.eq_ignore_ascii_case("eml:") => &rest[4..],
        _ => {
            return Err(
                "not an Energistics URI (Identifier Specification v4.0 §6.1: it begins with `eml:`)".into(),
            );
        }
    };
    let path = match rest.strip_prefix("//") {
        Some(authority_and_path) => authority_and_path
            .split_once('/')
            .map(|(_, path)| path)
            .unwrap_or(""),
        None => rest
            .strip_prefix('/')
            .ok_or("an Energistics URI's path begins with `/` (§6.1)")?,
    };
    for part in path.split('/') {
        if let Some(identifier) = part
            .strip_prefix("well(")
            .and_then(|inner| inner.strip_suffix(')'))
        {
            return Ok(Some(percent_decoded(identifier)));
        }
    }
    Ok(None)
}

/// RFC 3986 percent-decoding, the one the identifier grammar allows
/// (`pct-encoded`, §6.3). A malformed escape is kept as written.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(hex) = text.get(at + 1..at + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// Two definitive trajectories on one wellbore
// ---------------------------------------------------------------------------

/// WITSML TRG 2.1 §16.6 p. 350, on `Trajectory.Definitive`: *"There can only
/// be one trajectory per wellbore with definitive=true and it must define the
/// geometry of the whole wellbore (surface to bottom)."*
///
/// Judged per **wellbore**, and the finding is filed under the wellbore's uuid -
/// the object the rule is about, whether the package holds it or not - with
/// the trajectories named in what was read. One finding per wellbore, however
/// many trajectories claim it.
fn one_definitive_trajectory_per_wellbore(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    // Lower-cased wellbore uuid → (the uuid as first written, the claimants).
    let mut by_wellbore: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for (uuid, object) in context.objects {
        if object.type_info().type_key != "Trajectory"
            || text(*object, "Definitive").as_deref() != Some("true")
        {
            continue;
        }
        // A trajectory with no readable wellbore is `Validate`'s business: the
        // element is mandatory.
        let Some(wellbore) = reference(*object, "Wellbore") else {
            continue;
        };
        let title = identity::title_of_reflect(*object)
            .map(|title| format!(" ({title})"))
            .unwrap_or_default();
        by_wellbore
            .entry(wellbore.uuid.to_ascii_lowercase())
            .or_insert_with(|| (wellbore.uuid.clone(), Vec::new()))
            .1
            .push(format!("{uuid}{title}"));
    }
    for (wellbore, claimants) in by_wellbore.into_values() {
        if claimants.len() > 1 {
            out.push(violation(
                check,
                &wellbore,
                format!(
                    "{} trajectories say they are definitive: {}",
                    claimants.len(),
                    claimants.join(", ")
                ),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// IsListric on a fault that is not normal
// ---------------------------------------------------------------------------

/// RESQML Interpretations.xsd l. 659-662 (2.2) and the same annotation in
/// 2.0.1: *"Indicates if the normal fault is listric or not. BUSINESS RULE:
/// Must be present if the fault is normal. Must not be present if the fault is
/// not normal."* (TRG 2.2 §6.20 p. 188.)
///
/// **What "normal" means is the one reading here**, and it is the design's:
/// present if and only if the only set is normal - normal is the fault's **only** throw kind, across every
/// `ThrowInterpretation`. An oblique fault (`normal` and `strike-slip`) is
/// therefore not normal, and carries no `IsListric`.
///
/// Two cases are **not** judged:
///
/// - a fault that declares no throw at all: nothing says whether it is normal,
///   and an absent condition does not hold (the house reading of `when`);
/// - a thrust with `IsListric` false - the same page contradicts the business
///   rule: *"A thrust surface is specified as a FaultInterpretation whose
///   FaultThrow kind is "thrust" and which has the attributes: is Listric = 0"*.
///   Two sentences of one guide disagree, so the finding is *uncheckable* and
///   names both, rather than siding with either.
fn is_listric_iff_normal(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    for (uuid, object) in context.objects {
        if !concepts_of(*object).contains(&"FaultInterpretation") {
            continue;
        }
        let mut kinds: Vec<String> = Vec::new();
        for throw in objects(*object, "ThrowInterpretation") {
            for kind in scalars(throw, "Throw") {
                if !kinds.contains(&kind) {
                    kinds.push(kind);
                }
            }
        }
        if kinds.is_empty() {
            continue;
        }
        let normal = kinds.len() == 1 && kinds[0] == "normal";
        match (normal, text(*object, "IsListric")) {
            (true, Some(_)) | (false, None) => {}
            (true, None) => out.push(violation(
                check,
                uuid,
                "the fault's only throw kind is normal, and IsListric is absent".into(),
            )),
            (false, Some(listric)) if kinds.len() == 1 && kinds[0] == "thrust" && listric == "false" => {
                out.push(unverifiable(
                    check,
                    uuid,
                    "a thrust with IsListric false: TRG 2.2 §6.20 p. 188 asks for exactly that (\"is Listric = 0\") in its notes, and forbids it in the business rule on IsListric".into(),
                ))
            }
            (false, Some(listric)) => out.push(violation(
                check,
                uuid,
                format!("IsListric = {listric} on a fault whose throw is {}", kinds.join(" and ")),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Collections that are not what they say
// ---------------------------------------------------------------------------

/// One member of a collection, as the association names it.
struct Member {
    uuid: String,
    type_key: String,
}

/// Each collection's associations by lower-cased uuid: the version an
/// association names (`None`: none), and the members it lists.
type Memberships = BTreeMap<String, Vec<(Option<String>, Vec<Member>)>>;

/// What each collection holds, read from every
/// `CollectionsToDataobjectsAssociationSet` of the package - the only place
/// EML 2.3 writes membership (Collection.xsd: *"The relationships (between the
/// data objects and the collection) are specified and managed using the
/// SingleCollectionAssociation"*). Keyed by the lower-cased collection uuid,
/// each entry **carrying the version the association names** - two versions of
/// a collection hold two different member lists, and [`members_of`] tells them
/// apart.
///
/// The member's type is read off the reference, never by opening the member:
/// what a reference declares is what a reader of the collection is told.
fn memberships(context: &Context<'_>) -> Memberships {
    let mut out: Memberships = BTreeMap::new();
    for (_, object) in context.objects {
        if object.type_info().type_key != "CollectionsToDataobjectsAssociationSet" {
            continue;
        }
        for association in objects(*object, "SingleCollectionAssociation") {
            let Some(collection) = reference(association, "Collection") else {
                continue;
            };
            let version = field_version(association, "Collection");
            let members: Vec<Member> = objects(association, "Dataobject")
                .into_iter()
                .filter_map(reference_of)
                .collect();
            out.entry(collection.uuid.to_ascii_lowercase())
                .or_default()
                .push((version, members));
        }
    }
    out
}

/// `trg/homogeneous-collection`, on the collection's own flag. The finding is
/// the collection's: it is the object whose claim is false.
fn homogeneous_collection(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    let held = memberships(context);
    for (uuid, object) in context.objects {
        if object.type_info().type_key != "DataobjectCollection"
            || text(*object, "HomogeneousDatatype").as_deref() != Some("true")
        {
            continue;
        }
        let members = members_of(&held, uuid, identity::object_version_of_reflect(*object));
        if members.is_empty() {
            continue;
        }
        if let Some(read) = mixed(&members) {
            out.push(violation(
                check,
                uuid,
                format!("declares HomogeneousDatatype true and holds {read}"),
            ));
        }
    }
}

/// `trg/homogeneous-association`, on the association's own flag. Filed under
/// the association set, the association named by its index.
fn homogeneous_association(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    for (uuid, object) in context.objects {
        if object.type_info().type_key != "CollectionsToDataobjectsAssociationSet" {
            continue;
        }
        for (at, association) in objects(*object, "SingleCollectionAssociation")
            .into_iter()
            .enumerate()
        {
            if text(association, "HomogeneousDatatype").as_deref() != Some("true") {
                continue;
            }
            let members: Vec<Member> = objects(association, "Dataobject")
                .into_iter()
                .filter_map(reference_of)
                .collect();
            if let Some(read) = mixed(&members.iter().collect::<Vec<_>>()) {
                out.push(violation(
                    check,
                    uuid,
                    format!("SingleCollectionAssociation[{at}]: declares HomogeneousDatatype true and lists {read}"),
                ));
            }
        }
    }
}

/// The types among the members, counted, when there are several - `None` when
/// the members are of one type.
fn mixed(members: &[&Member]) -> Option<String> {
    let mut counted: Vec<(&str, usize)> = Vec::new();
    for member in members {
        match counted.iter_mut().find(|(key, _)| *key == member.type_key) {
            Some((_, count)) => *count += 1,
            None => counted.push((member.type_key.as_str(), 1)),
        }
    }
    (counted.len() > 1).then(|| {
        counted
            .iter()
            .map(|(key, count)| format!("{count} {key}"))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// `trg/wellbore-interpretation-set-holds-wellbores`. Filed under the earth
/// model, whose association the guide constrains; one finding for every
/// stranger across its sets, each named with the set it sits in.
fn wellbore_interpretation_set_holds_wellbores(
    context: &Context<'_>,
    check: &Integrity,
    out: &mut Vec<Finding>,
) {
    let held = memberships(context);
    for (uuid, object) in context.objects {
        if !concepts_of(*object).contains(&"EarthModelInterpretation") {
            continue;
        }
        let mut strangers = Vec::new();
        for (at, set) in objects(*object, "WellboreInterpretationSet")
            .into_iter()
            .enumerate()
        {
            let Some(collection) = reference_of(set) else {
                continue;
            };
            let members = members_of(&held, &collection.uuid, field_version_of(set));
            for member in members {
                if member.type_key != "WellboreInterpretation" {
                    strangers.push(format!(
                        "`WellboreInterpretationSet[{at}]` ({}) holds {} ({})",
                        collection.uuid, member.uuid, member.type_key
                    ));
                }
            }
        }
        if !strangers.is_empty() {
            out.push(violation(check, uuid, strangers.join(", ")));
        }
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

fn violation(check: &Integrity, uuid: &str, read: String) -> Finding {
    Finding {
        rule: check.id.to_owned(),
        uuid: uuid.to_owned(),
        says: check.says.to_owned(),
        outcome: Outcome::Violation { read },
        gravity: check.gravity,
    }
}

fn unverifiable(check: &Integrity, uuid: &str, why: String) -> Finding {
    Finding {
        rule: check.id.to_owned(),
        uuid: uuid.to_owned(),
        says: check.says.to_owned(),
        outcome: Outcome::Unverifiable { why },
        gravity: check.gravity,
    }
}

/// A scalar field as its wire text - an enumeration's value, `true` for a
/// boolean. `None` when absent.
fn text(object: &dyn Reflect, name: &str) -> Option<String> {
    match object.field(name)? {
        FieldValue::Scalar(scalar) => Some(scalar.to_string()),
        _ => None,
    }
}

/// A numeric scalar field.
fn number(object: &dyn Reflect, name: &str) -> Option<f64> {
    match object.field(name)? {
        FieldValue::Scalar(Scalar::Int(value)) => Some(value as f64),
        FieldValue::Scalar(Scalar::UInt(value)) => Some(value as f64),
        FieldValue::Scalar(Scalar::Double(value)) => Some(value),
        FieldValue::Scalar(Scalar::Str(value)) => value.trim().parse().ok(),
        _ => None,
    }
}

/// Every scalar of a field, repeated or not, as wire text.
fn scalars(object: &dyn Reflect, name: &str) -> Vec<String> {
    match object.field(name) {
        Some(FieldValue::Scalar(scalar)) => vec![scalar.to_string()],
        Some(FieldValue::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                FieldValue::Scalar(scalar) => Some(scalar.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Every object of a field, repeated or not.
fn objects<'a>(object: &'a dyn Reflect, name: &str) -> Vec<&'a dyn Reflect> {
    match object.field(name) {
        Some(FieldValue::Object(child)) => vec![child],
        Some(FieldValue::List(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                FieldValue::Object(child) => Some(child),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The reference held by a field, read with nothing opened.
fn reference(object: &dyn Reflect, name: &str) -> Option<Member> {
    match object.field(name)? {
        FieldValue::Object(child) => reference_of(child),
        _ => None,
    }
}

/// The members of the collection `uuid` at `version`: the associations that
/// name that version, and those that name none (a short reference is accepted
/// when nothing contradicts it). A collection asked for with no version takes
/// every association.
fn members_of<'a>(held: &'a Memberships, uuid: &str, version: Option<&str>) -> Vec<&'a Member> {
    held.get(&uuid.to_ascii_lowercase())
        .into_iter()
        .flatten()
        .filter(|(named, _)| identity::versions_agree(version, named.as_deref()))
        .flat_map(|(_, members)| members)
        .collect()
}

/// The version a reference held by a field names, when it names one.
fn field_version(object: &dyn Reflect, name: &str) -> Option<String> {
    match object.field(name)? {
        FieldValue::Object(child) => field_version_of(child).map(str::to_string),
        _ => None,
    }
}

fn field_version_of(reference: &dyn Reflect) -> Option<&str> {
    identity::object_version_of_reflect(reference)
        .map(str::trim)
        .filter(|version| !version.is_empty())
}

/// What a reference names: its uuid and the concept it declares.
fn reference_of(reference: &dyn Reflect) -> Option<Member> {
    let key = identity::key_of_reflect(reference)?;
    Some(Member {
        uuid: key.uuid.to_owned(),
        type_key: key.type_key().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_is_found_in_an_energistics_uri_and_nowhere_else() {
        // Identifier Specification v4.0 §6.3.1 p. 14's own example.
        assert_eq!(
            well_named_by("eml://prodml.mycompany.com/well(123456)/wellbore(01)/bottomhole/pressure meter"),
            Ok(Some("123456".into()))
        );
        // No authority (§6.2 allows it), and an escaped identifier.
        assert_eq!(
            well_named_by("eml:/well(GTH%2D9)/wellhead"),
            Ok(Some("GTH-9".into()))
        );
        // The scheme is not case sensitive (§6.1); `wellbore(…)` is not `well(…)`.
        assert_eq!(well_named_by("EML://a.b/wellbore(01)/wellhead"), Ok(None));
        // Not an Energistics URI: an error, which the check turns into an
        // uncheckable - never a pass.
        assert!(well_named_by("GTH-1 wellhead").is_err());
    }

    #[test]
    fn every_shipped_check_says_where_it_comes_from() {
        for check in checks() {
            assert!(
                check.id.starts_with("trg/") || check.id.starts_with("house/"),
                "`{}` does not say whether it is the standard's or ours",
                check.id
            );
            assert!(!check.says.trim().is_empty(), "`{}` says nothing", check.id);
        }
        let mut ids: Vec<&str> = checks().iter().map(|check| check.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), checks().len(), "two checks share an id");
    }
}
