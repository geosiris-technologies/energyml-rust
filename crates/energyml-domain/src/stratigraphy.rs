//! The order the package states - which surface is above which other one
//! (ADR-0014).
//!
//! # What the model says, and what it does not
//!
//! There is **no field** saying that a horizon is a unit's roof.
//! `ContactElementReference.Qualifier` does not serve: its values are
//! *footwall*, *hanging wall*, north/south/east/west - the sides of a fault.
//! What the model gives is an **order**: a
//! `StructuralOrganizationInterpretation` lists its horizons in an order,
//! and declares the **criterion** of that order.
//!
//! The three possible criteria all run from the top down: `age` is *"from
//! youngest to oldest"* (and the youngest is above), `apparent depth` *"from
//! the surface towards depth"*, `measured depth` *"from the wellhead towards
//! the bottom"*. So **an increasing index descends**, for all three - and
//! that is what makes the order directly usable. A criterion this build does
//! not know is not guessed: it is returned as it stands, and the caller
//! decides whether to use it.
//!
//!
//! # Why the provenance travels with the order
//!
//! Getting the criterion wrong **inverts every roof and every base at once**,
//! with nothing about the picture looking abnormal. An order whose source is
//! shown - which organisation, which criterion, which rank - can be disputed;
//! a mute order gets believed. That is what [`Placed`] exists for.
//!
//! # Two spellings, as always
//!
//! RESQML 2.0.1 says `Horizons` (`HorizonInterpretationIndex`es each carrying
//! its `Index`) and `OrderingCriteria`; 2.2 says
//! `OrderedBoundaryFeatureInterpretation` - where **the position in the list
//! is the order**, there is no `Index` any more - and
//! `AscendingOrderingCriteria`. Both lists live in named constants beside the
//! reader, the house rule: one more version is one more string.

use std::collections::HashMap;

use energyml_core::identity;
use energyml_core::path;
use energyml_core::value::{FieldValue, Reflect};

/// The spellings of the ordered list of horizons.
const ORDERED: &[&str] = &["Horizons", "OrderedBoundaryFeatureInterpretation"];
/// The spellings of the reference an entry of that list carries.
const ENTRY_REF: &[&str] = &["Horizon", "BoundaryFeatureInterpretation"];
/// The spellings of the ordering criterion.
const CRITERIA: &[&str] = &["OrderingCriteria", "AscendingOrderingCriteria"];
/// An entry's stratigraphic rank, when it carries one.
const RANK: &str = "StratigraphicRank";
/// An entry's explicit index - 2.0.1 only; in 2.2 it is the position in the
/// list.
const INDEX: &str = "Index";

/// Where an interpretation stands in an organisation's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// The index in the order - **increasing downwards** for the three
    /// criteria the schema defines.
    pub index: i64,
    /// The stratigraphic rank, when the entry declares one.
    pub rank: Option<i64>,
    /// The uuid of the organisation that states it - the provenance, to show.
    pub organisation: String,
    /// The criterion, as the file writes it (`age`, `apparent depth`, ...).
    pub criteria: String,
}

impl Placed {
    /// Is this criterion one of the three the schema defines, and therefore
    /// one known to descend?
    ///
    /// An unknown criterion (a value from a future version, or an `Other`
    /// from an open enumeration) is not *assumed* to descend: the order is
    /// returned all the same, but the caller must be able to refuse to use
    /// it rather than return an inverted section without knowing.
    pub fn descends(&self) -> bool {
        matches!(self.criteria.trim(), "age" | "apparent depth" | "measured depth")
    }
}

/// The order read from every organisation of a package.
#[derive(Clone, Debug, Default)]
pub struct Order {
    /// By interpretation uuid, lowercased - a uuid is written in both cases
    /// in real packages.
    placed: HashMap<String, Placed>,
    /// What could not be read, named - never a silence.
    pub notes: Vec<String>,
}

impl Order {
    /// Where this **interpretation** stands, if an organisation says so.
    pub fn of_interpretation(&self, uuid: &str) -> Option<&Placed> {
        self.placed.get(&uuid.to_ascii_lowercase())
    }

    /// How many interpretations are placed.
    pub fn len(&self) -> usize {
        self.placed.len()
    }

    /// No organisation placed anything - the frequent case, and an answer.
    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }
}

/// The first field found among several spellings.
fn field_any<'a>(object: &'a dyn Reflect, names: &[&str]) -> Option<FieldValue<'a>> {
    names.iter().find_map(|name| object.field(name))
}

/// A field's lexical value, whatever its spelling.
fn text_any(object: &dyn Reflect, names: &[&str]) -> Option<String> {
    match field_any(object, names)? {
        FieldValue::Scalar(scalar) => Some(scalar.to_string()),
        _ => None,
    }
}

/// An integer from a named field, or `None` - an absent field is not zero.
fn integer(object: &dyn Reflect, name: &str) -> Option<i64> {
    match object.field(name)? {
        FieldValue::Scalar(scalar) => scalar.to_string().trim().parse().ok(),
        _ => None,
    }
}

/// Read the order a package's structural organisations state.
///
/// `objects` is everything the package holds: the organisations are
/// recognised by their `type_key`, so there is nothing for the caller to
/// filter and no list of versions to keep.
///
/// Two organisations placing the same interpretation in two places are **not
/// arbitrated**: the first wins and the disagreement is noted. Silently
/// choosing between two structural models would mean preferring one without
/// saying so, and that is exactly the kind of choice that must reach a
/// human.
pub fn read<'a>(objects: impl Iterator<Item = &'a dyn Reflect>) -> Order {
    let mut out = Order::default();
    for object in objects {
        if object.type_info().type_key != "StructuralOrganizationInterpretation" {
            continue;
        }
        let organisation = identity::uuid_of_reflect(object).unwrap_or_default().to_owned();
        let criteria = text_any(object, CRITERIA).unwrap_or_default();
        if criteria.is_empty() {
            // With no criterion the order exists but its **direction** is
            // unknown: reading it anyway and saying so beats keeping quiet,
            // but `descends()` will refuse to assert it.
            out.notes.push(format!(
                "organisation {organisation} states no ordering criterion - the direction of its order is unknown"
            ));
        }
        // An absent field and an **empty** list are two different pieces
        // of news, but they answer the same thing to the question asked
        // - *where is my surface in the order?* - so both are stated:
        // that is what explains why no surface was placed.
        let entries = match field_any(object, ORDERED) {
            Some(FieldValue::List(entries)) if !entries.is_empty() => entries,
            _ => {
                out.notes.push(format!(
                    "organisation {organisation} lists no ordered horizon"
                ));
                continue;
            }
        };
        for (position, entry) in entries.iter().enumerate() {
            let FieldValue::Object(entry) = entry else {
                continue;
            };
            let Some(FieldValue::Object(reference)) = field_any(*entry, ENTRY_REF) else {
                continue;
            };
            let Some(uuid) = identity::uuid_of_reflect(reference).filter(|u| !u.is_empty()) else {
                out.notes.push(format!(
                    "organisation {organisation} references a horizon with no uuid at position {position}"
                ));
                continue;
            };
            // The explicit 2.0.1 index, or the **position** in 2.2 - where the
            // order *is* the list's, `Index` having disappeared.
            let index = integer(*entry, INDEX).unwrap_or(position as i64);
            let placed = Placed {
                index,
                rank: integer(*entry, RANK),
                organisation: organisation.clone(),
                criteria: criteria.clone(),
            };
            let key = uuid.to_ascii_lowercase();
            if let Some(already) = out.placed.get(&key) {
                if already.organisation != placed.organisation {
                    out.notes.push(format!(
                        "interpretation {uuid} is placed by two organisations ({} and {}) - the first is kept, the disagreement belongs to a human",
                        already.organisation, placed.organisation
                    ));
                }
                continue;
            }
            out.placed.insert(key, placed);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// What the package **states** about unconformities
// ---------------------------------------------------------------------------

/// The contacts field, carried by organisations as by stratigraphic
/// columns.
const CONTACTS: &str = "ContactInterpretation";
/// An horizon interpretation's boundary relation - the same spelling in both
/// versions, an **open** enumeration in 2.2 (hence a `String`).
const BOUNDARY: &str = "BoundaryRelation";
/// The mode of one side of a contact - `SecondaryQualifier` in both
/// versions, on a `ContactElementReference` (2.0.1: `baselap`, `erosion`,
/// `extended`, `proportional`) or a `ContactElement` (2.2: `conformable`,
/// `unconformable`, `extended`). An earlier comment here said 2.2 had removed
/// it; the 2.2 XSD keeps it (`ContactElement.SecondaryQualifier`, 24/09).
const MODE: &str = "SecondaryQualifier";

/// A stated contact: *subject verb object*, with the declared modes.
#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    /// The uuid of the subject interpretation.
    pub subject: String,
    /// The verb, as the file writes it - `erodes`, `interrupts`, ...
    pub verb: String,
    /// The uuid of the object interpretation.
    pub object: String,
    /// The modes declared on either side - `erosion`, `extended`, ...
    pub modes: Vec<String>,
}

/// What the package says about unconformities, ready to be **confronted**
/// with the geometry.
///
/// The document's rule (section 5): read, measure, confront. This type
/// carries the first half; the second is the crossing `fence` measures. A
/// disagreement between the two is **named**, never arbitrated: inferring an
/// erosion from a crossing when the file declares one is guessing where there
/// is a fact; ignoring it when the file is silent is refusing to see.
///
#[derive(Clone, Debug, Default)]
pub struct Statements {
    relations: HashMap<String, Vec<String>>,
    contacts: Vec<Contact>,
}

/// The verbs and modes that **explain** one surface crossing another.
/// `extended` is here because the model has a word for the horizon stretched
/// at the edge, which is the other ordinary cause of a crossing.
const EXPLAINS: &[&str] = &["erodes", "interrupts", "erosion", "extended", "baselap"];

impl Statements {
    /// What the package states that could explain a crossing between these two
    /// interpretations. Empty: it says nothing about it - which is an answer, and
    /// the one that calls for a human.
    pub fn explains(&self, first: &str, second: &str) -> Vec<String> {
        let (a, b) = (first.to_ascii_lowercase(), second.to_ascii_lowercase());
        let mut out = Vec::new();
        for contact in &self.contacts {
            let pair = (contact.subject.eq_ignore_ascii_case(&a) && contact.object.eq_ignore_ascii_case(&b))
                || (contact.subject.eq_ignore_ascii_case(&b) && contact.object.eq_ignore_ascii_case(&a));
            if !pair {
                continue;
            }
            let says = std::iter::once(contact.verb.as_str())
                .chain(contact.modes.iter().map(String::as_str))
                .filter(|word| EXPLAINS.iter().any(|known| word.eq_ignore_ascii_case(known)))
                .collect::<Vec<_>>();
            if !says.is_empty() {
                out.push(format!(
                    "the package states a contact \"{} {} {}\" ({})",
                    contact.subject,
                    contact.verb,
                    contact.object,
                    says.join(", ")
                ));
            }
        }
        for uuid in [&a, &b] {
            for relation in self.relations.get(uuid).into_iter().flatten() {
                if relation.to_ascii_lowercase().contains("unconformable") {
                    out.push(format!("interpretation {uuid} is declared \"{relation}\""));
                }
            }
        }
        out
    }

    /// How many contacts were read - enough to say that a package states none,
    /// which is not the same thing as "none applies".
    ///
    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }
}

/// A field's lexical value, whatever its scalar type.
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

/// Read what the package states about unconformities.
///
/// Contacts are carried by structural organisations **and** by stratigraphic
/// columns: rather than keeping the list of types that have them, every
/// object carrying the field is taken - one more type in a future version
/// enters with nothing changed here.
pub fn statements<'a>(objects: impl Iterator<Item = &'a dyn Reflect>) -> Statements {
    let mut out = Statements::default();
    for object in objects {
        let relations = scalars(object, BOUNDARY);
        if !relations.is_empty()
            && let Some(uuid) = identity::uuid_of_reflect(object).filter(|u| !u.is_empty())
        {
            out.relations.insert(uuid.to_ascii_lowercase(), relations);
        }
        let Some(FieldValue::List(parts)) = object.field(CONTACTS) else {
            continue;
        };
        for part in parts {
            let FieldValue::Object(part) = part else { continue };
            // Reflection crosses a choice: the caller sees the concrete subtype,
            // so `Subject`/`Verb`/`DirectObject` read on it.
            let (Some(FieldValue::Object(subject)), Some(FieldValue::Object(target))) =
                (part.field("Subject"), part.field("DirectObject"))
            else {
                continue;
            };
            let (Some(from), Some(to)) = (
                identity::uuid_of_reflect(subject).filter(|u| !u.is_empty()),
                identity::uuid_of_reflect(target).filter(|u| !u.is_empty()),
            ) else {
                continue;
            };
            let mut modes = scalars(subject, MODE);
            modes.extend(scalars(target, MODE));
            out.contacts.push(Contact {
                subject: from.to_ascii_lowercase(),
                verb: scalars(part, "Verb").first().cloned().unwrap_or_default(),
                object: to.to_ascii_lowercase(),
                modes,
            });
        }
    }
    out
}

/// The spellings of a representation's reference to what it represents -
/// 2.0.1 and 2.2 do not write it the same way.
const REPRESENTS: &[&str] = &["RepresentedInterpretation", "RepresentedObject"];

/// The uuid of the interpretation a representation represents, or `None`.
pub fn interpretation_of(object: &dyn Reflect) -> Option<String> {
    for name in REPRESENTS {
        if let Ok(FieldValue::Object(reference)) = path::get(object, name)
            && let Some(uuid) = identity::uuid_of_reflect(reference).filter(|u| !u.is_empty())
        {
            return Some(uuid.to_owned());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The stratigraphic column itself
// ---------------------------------------------------------------------------

/// The ranks field, carried by `StratigraphicColumn` - the same spelling in
/// both versions.
const RANKS: &str = "Ranks";
/// The ordered list of a rank's units - the same spelling, two **shapes**.
const UNITS: &str = "StratigraphicUnits";
/// The 2.0.1 wrapper around the reference: a
/// `StratigraphicUnitInterpretationIndex { Index, Unit }`. In 2.2 the entry
/// **is** the reference and the position orders it.
const UNIT_REF: &str = "Unit";
/// The rank's number in the column - `RankInStratigraphicColumn` in 2.2,
/// `Index` in 2.0.1. The order matters: `Index` exists on entries too.
const RANK_NUMBER: &[&str] = &["RankInStratigraphicColumn", "Index"];
/// A unit's stratigraphic register - 2.2 only (`chronostratigraphic`,
/// `lithostratigraphic`, ...). It is what names the *column* of an
/// InfoTerre-style table without deducing it from the rank number.
const ROLE: &str = "StratigraphicRole";
/// What a contact is part of. The XSD documents nothing about it; the
/// reference package puts the **horizon** between the two units there, in
/// both versions. Returned as-is, with its type, not renamed "surface".
const PART_OF: &str = "PartOf";
/// The nature of the contact - 2.0.1 only ("stratigraphic unit to
/// stratigraphic unit", "horizon to fault", ...). Removed in 2.2.
const RELATIONSHIP: &str = "ContactRelationship";

/// A reference as read: enough to show it and to find it again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Named {
    /// The uuid, lowercased - a uuid is written in both cases.
    pub uuid: String,
    /// **The uri** of the object the reference names: the held object's own
    /// identity (its type and the version it declares) when the package holds
    /// it, the reference's own address otherwise — its uri when it names a type,
    /// the uuid when it names none. What a view opens and keys by: two versions
    /// of one object are two uris with one uuid.
    pub uri: String,
    /// The title **the reference cached**, or the object's when it is there.
    /// The two may differ, and the second wins.
    pub title: String,
    /// The target's `type_key` when it is in the package, otherwise what the
    /// reference declares. Empty when nothing says.
    pub kind: String,
}

/// A unit, in its place in a rank.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnUnit {
    /// The unit itself.
    pub unit: Named,
    /// What **orders**: the declared `Index` in 2.0.1, the position in 2.2.
    pub index: i64,
    /// The register - `chronostratigraphic`, `lithostratigraphic`, ... when
    /// the unit declares one (2.2). `None` is not "chrono by default".
    pub role: Option<String>,
    /// Is the unit absent from the package? A dangling reference is drawn all
    /// the same - removing it would make a holed column that says nothing.
    pub resolved: bool,
    /// What **represents** this unit, in the package.
    ///
    /// The reference runs the other way - it is the representation that names
    /// its interpretation - so it is found by sweeping the objects, never by
    /// following a field of the unit. That is what makes the column a way into
    /// the package rather than a diagram beside it.
    pub represented: Vec<Named>,
    /// **Its age span**, `[old, young]` in millions of years before present,
    /// when the unit states one as numbers — a `GeologicTimeBasedTimeInterval`
    /// in its `HasOccurredDuring` (RESQML 2.2 §6.47).
    ///
    /// `None` is the ordinary case: not one package of the corpus dates its
    /// units (24/09), and a 2.0.1 unit bounds its time by two **horizons**,
    /// which are not numbers. What a view does with it — aligning two ranks by
    /// the interval that contains another — is only honest where the file said
    /// it, so nothing is derived here.
    pub age: Option<[f64; 2]>,
    /// **The earth models that hold this unit** — made *for* it (`root`) or
    /// merely containing it — found by walking the references backwards:
    /// unit ← rank ← column ← `EarthModelInterpretation`
    /// Filled in the same
    /// pass as the column, so a view asks nothing per card.
    pub models: Vec<ModelOf>,
}

/// An earth model a unit belongs to, as a column card lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelOf {
    /// The `EarthModelInterpretation`.
    pub emi: Named,
    /// The `Model` it interprets — empty when its `InterpretedFeature` is.
    pub model: Named,
    /// **Made for this unit**: the unit is the only one of the coarsest rank of
    /// the model's column. Otherwise the model only *contains* it — a Paleogene
    /// model contains the Eocene, and the two answer different questions.
    pub root: bool,
    /// `Citation/Creation`, verbatim — what orders two snapshots of one model.
    pub created: String,
    /// What the model carries besides its column.
    pub structure: bool,
    /// Whether it names fluid organisations.
    pub fluids: bool,
    /// Whether it names a wellbore set.
    pub wells: bool,
}

/// A contact stated **within** a rank, as the file writes it.
#[derive(Clone, Debug, PartialEq)]
pub struct RankContact {
    /// The sentence's subject.
    pub subject: Named,
    /// Its `SecondaryQualifier`: `proportional`/`erosion`/... in 2.0.1,
    /// `conformable`/`unconformable`/`extended` in 2.2.
    pub subject_mode: Option<String>,
    /// The verb, verbatim - `stops at` in 2.0.1, `stops` in 2.2.
    pub verb: String,
    /// The direct object.
    pub object: Named,
    /// Its `SecondaryQualifier`.
    pub object_mode: Option<String>,
    /// `PartOf`, when there is one.
    pub part_of: Option<Named>,
    /// `ContactRelationship` - 2.0.1 only.
    pub relationship: Option<String>,
    /// The sides of the contact the file says are **not conformable** — an
    /// erosion, an onlap — each named by the unit on that side, with the
    /// statement that says so. Empty is *nothing stated*, not *conformable*.
    pub unconformable: Vec<Unconformable>,
}

/// One side of a contact the file states is not conformable.
///
/// Named by the **unit** on that side rather than by *above* or *below*: the
/// direction of a contact is not data (see [`RankContact::joins`]), and a view
/// that draws the rank in another order than the list — by age — would put
/// an *above* on the wrong side.
///
/// Four statements say it, two per release, and none is invented here:
///
/// - the side's own `SecondaryQualifier` — `erosion` in 2.0.1, `unconformable`
///   in 2.2 (the unit so qualified is the one that stops against the other
///   without conforming to it);
/// - the horizon the contact is `PartOf` — `IsConformableAbove` /
///   `IsConformableBelow` set to `false` in 2.2, a `BoundaryRelation` of
///   `unconformable above` / `below` / `below and above` in 2.0.1. Those speak
///   of *above* and *below* the horizon, which this reader turns into a unit
///   through the rank's order — and only when the criterion is one that
///   descends; otherwise it says nothing.
///
/// `baselap` (2.0.1) is left out: it is an onlap or a downlap, a geometry of
/// the beds against the surface, not a surface cutting them.
#[derive(Clone, Debug, PartialEq)]
pub struct Unconformable {
    /// The **uri** of the unit on that side ([`Named::uri`]).
    pub unit: String,
    /// What the file says, verbatim enough to be found in it.
    pub says: String,
}

impl RankContact {
    /// Does this contact concern **these two** units, one way or the other?
    ///
    /// One way or the other because the direction is not data: the reference
    /// package writes the same contact `Subject=B, Object=A` in 2.0.1 and
    /// `Subject=A, Object=B` in 2.2. Treating the subject as "the one above"
    /// would therefore invert half the packages.
    pub fn joins(&self, first: &str, second: &str) -> bool {
        let (a, b) = (first.to_ascii_lowercase(), second.to_ascii_lowercase());
        (self.subject.uuid == a && self.object.uuid == b) || (self.subject.uuid == b && self.object.uuid == a)
    }
}

/// A column's rank: one level of detail, its units in order.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnRank {
    /// The rank itself.
    pub rank: Named,
    /// Its number in the column, when it declares one.
    pub number: Option<i64>,
    /// The ordering criterion, verbatim - `OrderingCriteria` /
    /// `AscendingOrderingCriteria`. `minOccurs="1"` in both versions, so empty
    /// means *the file did not write it*.
    pub criteria: String,
    /// The units, **from the top down**.
    pub units: Vec<ColumnUnit>,
    /// What the rank states between its units.
    pub contacts: Vec<RankContact>,
    /// What could not be read, named.
    pub notes: Vec<String>,
}

impl ColumnRank {
    /// Does an increasing index descend, for this criterion?
    ///
    /// Same rule as [`Placed::descends`], and the same reason not to guess it:
    /// the schema's three criteria all descend, a fourth this build does not
    /// know is not *assumed* to.
    pub fn descends(&self) -> bool {
        matches!(self.criteria.trim(), "age" | "apparent depth" | "measured depth")
    }

    /// The contact the rank states between two consecutive units, if there is
    /// one. Several contacts for one pair are all returned: keeping only one, as
    /// the tool this reader takes after does, hides the second without a word.
    ///
    pub fn between(&self, above: &str, below: &str) -> Vec<&RankContact> {
        self.contacts.iter().filter(|c| c.joins(above, below)).collect()
    }
}

/// A stratigraphic column and its ranks.
#[derive(Clone, Debug, PartialEq)]
pub struct StratigraphicColumn {
    /// The column itself.
    pub column: Named,
    /// Its ranks, coarsest to finest when they are numbered.
    pub ranks: Vec<ColumnRank>,
    /// The `EarthModelInterpretation`s that name this column as theirs. A
    /// column a model owns is shown **in the model's tab**, not among the
    /// package's own.
    pub models: Vec<Named>,
}

/// Everything a package holds of ordered stratigraphy.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Columns {
    /// The declared columns.
    pub columns: Vec<StratigraphicColumn>,
    /// The ranks **no** column lists.
    ///
    /// They exist: a `StratigraphicColumnRankInterpretation` is a top-level
    /// object and nothing obliges a column to reference it. Keeping quiet
    /// about them would make a stratigraphy the package contains disappear
    /// from the screen - the rule "nothing leaves the inventory in silence".
    pub loose_ranks: Vec<ColumnRank>,
    /// What could not be read, named.
    pub notes: Vec<String>,
}

/// The first non-empty scalar of a field, or `None`.
fn text(object: &dyn Reflect, name: &str) -> Option<String> {
    match object.field(name)? {
        FieldValue::Scalar(scalar) => Some(scalar.to_string()).filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// The objects of a package, by uuid — and **every version of a uuid** the
/// package holds, so that a reference finds the one it names
/// ([`Held::of`]) instead of whichever came last.
#[derive(Default)]
struct Held<'a> {
    /// What a bare uuid names: the last object held with it.
    by_uuid: HashMap<String, &'a dyn Reflect>,
    /// Every object of a uuid, in the order held.
    versions: HashMap<String, Vec<&'a dyn Reflect>>,
}

impl<'a> FromIterator<(String, &'a dyn Reflect)> for Held<'a> {
    fn from_iter<T: IntoIterator<Item = (String, &'a dyn Reflect)>>(pairs: T) -> Self {
        let mut held = Held::default();
        for (uuid, object) in pairs {
            held.by_uuid.insert(uuid.clone(), object);
            held.versions.entry(uuid).or_default().push(object);
        }
        held
    }
}

impl<'a> Held<'a> {
    /// The object a bare uuid names.
    fn get(&self, uuid: &str) -> Option<&&'a dyn Reflect> {
        self.by_uuid.get(uuid)
    }

    /// **The object a reference names**: of the objects held under its uuid,
    /// the one at the version it names (`identity::version_fit`, the one rule —
    /// the exact version, then one that contradicts nothing). A uuid held once
    /// answers whatever version the reference spells, as the store's lookup
    /// does (*a wrong version is not a wrong object*, the checker reports it);
    /// a uuid held twice whose versions the reference contradicts answers
    /// `None`: guessing would show the wrong version under the right name.
    fn of(&self, reference: &dyn Reflect) -> Option<&'a dyn Reflect> {
        let uuid = identity::uuid_of_reflect(reference)?.to_ascii_lowercase();
        let all = self.versions.get(&uuid)?;
        if let [only] = all.as_slice() {
            return Some(*only);
        }
        let wanted = identity::object_version_of_reflect(reference);
        let mut best: Option<(identity::VersionFit, &'a dyn Reflect)> = None;
        for object in all {
            let fit = identity::version_fit(wanted, identity::object_version_of_reflect(*object));
            if fit == identity::VersionFit::Contradicts {
                continue;
            }
            if best.is_none_or(|(kept, _)| fit < kept) {
                best = Some((fit, *object));
            }
        }
        best.map(|(_, object)| object)
    }
}

/// Read a reference, preferring what the **target object** says about itself.
///
/// The title cached in a `DataObjectReference` may be stale - it is written
/// once and nothing refreshes it when the target is renamed. When the target
/// is in the package, it is the target that is authoritative; otherwise the
/// cache is all there is, and it beats a bare uuid.
fn named(reference: &dyn Reflect, held: &Held<'_>) -> Named {
    let uuid = identity::uuid_of_reflect(reference)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let cached = identity::title_of_reflect(reference)
        .unwrap_or_default()
        .to_owned();
    let address = identity::address_of_reflect(reference);
    match held.of(reference) {
        Some(target) => Named {
            title: identity::title_of_reflect(target)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .unwrap_or(cached),
            kind: target.type_info().type_key.to_owned(),
            // The identity of the object found — the version it declares — not
            // what the reference spelt.
            uri: identity::uri_of_reflect(target).or(address).unwrap_or_else(|| uuid.clone()),
            uuid,
        },
        None => Named {
            uri: address.unwrap_or_else(|| uuid.clone()),
            uuid,
            title: cached,
            // Nothing in the package: the type is the one the reference
            // **declares**, read by the same key as everywhere else.
            kind: identity::key_of_reflect(reference)
                .map(|key| key.type_key().to_owned())
                .unwrap_or_default(),
        },
    }
}

/// An organisation's contacts, read **inside the object**.
///
/// An `AbstractContactInterpretationPart` is a field, not a top-level
/// object: it has neither uuid nor uri, and looking for it in a graph of
/// references never finds it. That is the gap this reader fills.
fn contacts_of(object: &dyn Reflect, held: &Held<'_>) -> Vec<RankContact> {
    let Some(FieldValue::List(parts)) = object.field(CONTACTS) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for part in parts {
        let FieldValue::Object(part) = part else { continue };
        // Reflection crosses the choice: the concrete subtype is what is seen,
        // so a `MultipleContactInterpretationPart` simply has no such fields
        // and is skipped - it names no pair, it aggregates binary contacts by
        // their `Index`.
        let (Some(FieldValue::Object(subject)), Some(FieldValue::Object(target))) =
            (part.field("Subject"), part.field("DirectObject"))
        else {
            continue;
        };
        out.push(RankContact {
            subject: named(subject, held),
            subject_mode: text(subject, MODE),
            verb: text(part, "Verb").unwrap_or_default(),
            object: named(target, held),
            object_mode: text(target, MODE),
            part_of: match part.field(PART_OF) {
                Some(FieldValue::Object(reference)) => Some(named(reference, held)),
                _ => None,
            },
            relationship: text(part, RELATIONSHIP),
            unconformable: Vec::new(),
        });
    }
    out
}

/// The `SecondaryQualifier` values that state a side is not conformable.
const UNCONFORMABLE_MODES: &[&str] = &["unconformable", "erosion"];

/// Fill each contact's [`Unconformable`] sides, once the rank's order is known.
///
/// `position` gives each unit's place in the rank, top first; `None` when the
/// criterion does not say which way the list runs — then *above* and *below*
/// a horizon cannot be turned into a unit, and only the sides' own modes are
/// read.
fn read_unconformities(
    contacts: &mut [RankContact],
    position: Option<&HashMap<String, usize>>,
    held: &Held<'_>,
) {
    for contact in contacts {
        let mut sides = Vec::new();
        for (side, mode) in [(&contact.subject, &contact.subject_mode), (&contact.object, &contact.object_mode)] {
            if let Some(mode) = mode.as_deref().filter(|mode| UNCONFORMABLE_MODES.contains(&mode.trim())) {
                sides.push(Unconformable {
                    unit: side.uri.clone(),
                    says: format!("{} is qualified \"{}\" in the contact", side.title, mode.trim()),
                });
            }
        }
        let horizon = contact.part_of.as_ref().and_then(|part| held.get(&part.uuid).map(|object| (part, *object)));
        if let (Some((part, horizon)), Some(position)) = (horizon, position) {
            let at = |named: &Named| position.get(&named.uuid).copied();
            if let (Some(a), Some(b)) = (at(&contact.subject), at(&contact.object)) {
                let (above, below) =
                    if a < b { (&contact.subject, &contact.object) } else { (&contact.object, &contact.subject) };
                let flag = |name: &str| match horizon.field(name) {
                    Some(FieldValue::Scalar(value)) => Some(value.to_string() == "false"),
                    _ => None,
                };
                let relations: Vec<String> = match horizon.field("BoundaryRelation") {
                    Some(FieldValue::List(values)) => values
                        .iter()
                        .filter_map(|value| match value {
                            FieldValue::Scalar(value) => Some(value.to_string()),
                            _ => None,
                        })
                        .collect(),
                    Some(FieldValue::Scalar(value)) => vec![value.to_string()],
                    _ => Vec::new(),
                };
                let relation = |which: &str| {
                    relations.iter().any(|r| r == &format!("unconformable {which}") || r == "unconformable below and above")
                };
                for (unit, flag_name, which) in [(above, "IsConformableAbove", "above"), (below, "IsConformableBelow", "below")] {
                    let says = if flag(flag_name) == Some(true) {
                        Some(format!("{} states {flag_name} = false", part.title))
                    } else if relation(which) {
                        Some(format!("{} states BoundaryRelation \"unconformable {which}\"", part.title))
                    } else {
                        None
                    };
                    if let Some(says) = says {
                        sides.push(Unconformable { unit: unit.uri.clone(), says });
                    }
                }
            }
        }
        contact.unconformable = sides;
    }
}

/// A unit's age span, `[old, young]` in Ma, when it states one as numbers.
///
/// `HasOccurredDuring` → `GeologicTimeBasedTimeInterval` → `Start` and `End`,
/// each a `GeologicTime`: a `DateTime` **plus** an `AgeOffsetAttribute` in
/// years, positive into the past (eml 2.3). The offset over a million is the
/// age; the reference instant moves it by a few decades, which is below what a
/// stage boundary is known to. An interval bounded by horizons (the other
/// `AbstractTimeInterval`) has no number and gives `None`. `Start` is the old
/// end — an interval runs forwards in time — and the pair comes back ordered
/// whatever the file wrote, since a reversed interval is a slip of the writer,
/// not a different age.
fn age_of(unit: &dyn Reflect) -> Option<[f64; 2]> {
    let FieldValue::Object(interval) = unit.field("HasOccurredDuring")? else { return None };
    let offset = |end: &str| -> Option<f64> {
        let FieldValue::Object(time) = interval.field(end)? else { return None };
        integer(time, "AgeOffsetAttribute").map(|years| years as f64 / 1_000_000.0)
    };
    let (start, end) = (offset("Start")?, offset("End")?);
    Some([start.max(end), start.min(end)])
}

/// Read a `StratigraphicColumnRankInterpretation`.
///
/// # Where the order comes from, and where it does not
///
/// Not from the contacts: `ContactVerb` reduces to `stops`, `splits`,
/// `crosses` in 2.2, and none of them says which is above. The order is the
/// one the list states - the declared `Index` of 2.0.1, the position in 2.2,
/// where `StratigraphicUnits` is a plain run of references - and its
/// **direction** comes from the criterion, descending for all three values.
fn read_rank(
    object: &dyn Reflect,
    held: &Held<'_>,
    represents: &HashMap<String, Vec<Named>>,
) -> ColumnRank {
    let mut notes = Vec::new();
    let rank = named(object, held);
    let criteria = text_any(object, CRITERIA).unwrap_or_default();
    if criteria.is_empty() {
        notes.push(format!(
            "rank {} states no ordering criterion - the direction of the stack is unknown",
            rank.uuid
        ));
    }

    let entries = match object.field(UNITS) {
        Some(FieldValue::List(entries)) => entries,
        Some(other) => vec![other],
        None => Vec::new(),
    };
    let mut units = Vec::with_capacity(entries.len());
    for (position, entry) in entries.iter().enumerate() {
        let FieldValue::Object(entry) = entry else {
            continue;
        };
        // 2.0.1 wraps the reference in a `...Index { Index, Unit }`; 2.2 puts
        // the reference directly. The wrapper is recognised by its `Unit`
        // field, never by the version.
        let (reference, index) = match entry.field(UNIT_REF) {
            Some(FieldValue::Object(inner)) => (
                inner,
                // The declared `Index` is authoritative. The position only
                // serves when it is missing - the document order is not
                // normative in 2.0.1, the index is.
                integer(*entry, INDEX).unwrap_or(position as i64),
            ),
            _ => (*entry, position as i64),
        };
        let unit = named(reference, held);
        // The object the reference names, at the version it names.
        let target = held.of(reference);
        let resolved = target.is_some();
        if !resolved && !unit.uuid.is_empty() {
            notes.push(format!(
                "unit {} ({}) is not in the package - drawn from the reference alone",
                unit.uuid, unit.title
            ));
        }
        units.push(ColumnUnit {
            role: target.and_then(|target| text(target, ROLE)),
            age: target.and_then(age_of),
            represented: represents.get(&unit.uuid).cloned().unwrap_or_default(),
            index,
            unit,
            resolved,
            models: Vec::new(),
        });
    }
    // Sorted by the declared index, keeping the document order on ties: two
    // entries with the same `Index` are a producer's fault, and picking one
    // at random would place a unit on the wrong storey.
    let mut seen: HashMap<i64, usize> = HashMap::new();
    for unit in &units {
        *seen.entry(unit.index).or_insert(0) += 1;
    }
    for (index, count) in seen.iter().filter(|(_, count)| **count > 1) {
        notes.push(format!(
            "rank {} gives index {index} to {count} units - the document order settles it, and it is a fault of the file",
            rank.uuid
        ));
    }
    units.sort_by_key(|unit| unit.index);

    let mut contacts = contacts_of(object, held);
    let descends = matches!(criteria.trim(), "age" | "apparent depth" | "measured depth");
    let position: HashMap<String, usize> =
        units.iter().enumerate().map(|(at, unit)| (unit.unit.uuid.clone(), at)).collect();
    read_unconformities(&mut contacts, descends.then_some(&position), held);

    ColumnRank {
        number: RANK_NUMBER.iter().find_map(|name| integer(object, name)),
        rank,
        criteria,
        contacts,
        units,
        notes,
    }
}

/// A package's stratigraphic columns, each with its ranks.
///
/// `objects` is everything the package holds: the columns, ranks and units
/// are recognised by their `type_key`, so there is nothing for the caller to
/// filter and no list of versions to keep.
///
/// Empty when the package has no column, which is the common case - and not
/// a failure to report.
pub fn columns<'a>(objects: impl Iterator<Item = &'a dyn Reflect>) -> Columns {
    let all: Vec<&dyn Reflect> = objects.collect();
    let held: Held<'_> = all
        .iter()
        .filter_map(|object| {
            identity::uuid_of_reflect(*object)
                .filter(|uuid| !uuid.is_empty())
                .map(|uuid| (uuid.to_ascii_lowercase(), *object))
        })
        .collect();

    // What represents what, once for the whole package. The reference runs
    // from the representation to the interpretation, so the index is built by
    // sweeping - doing it per unit would be quadratic on a long column.
    let mut represents: HashMap<String, Vec<Named>> = HashMap::new();
    for object in &all {
        let Some(uuid) = interpretation_of(*object) else {
            continue;
        };
        represents
            .entry(uuid.to_ascii_lowercase())
            .or_default()
            .push(named(*object, &held));
    }

    let mut out = Columns::default();
    let mut listed: Vec<String> = Vec::new();

    for object in &all {
        if object.type_info().type_key != "StratigraphicColumn" {
            continue;
        }
        let column = named(*object, &held);
        let references = match object.field(RANKS) {
            Some(FieldValue::List(references)) => references,
            Some(other) => vec![other],
            None => Vec::new(),
        };
        let mut ranks = Vec::new();
        for reference in &references {
            let FieldValue::Object(reference) = reference else {
                continue;
            };
            let uuid = identity::uuid_of_reflect(*reference)
                .unwrap_or_default()
                .to_ascii_lowercase();
            listed.push(uuid.clone());
            match held.get(&uuid) {
                Some(target) => ranks.push(read_rank(*target, &held, &represents)),
                None => out.notes.push(format!(
                    "column {} lists rank {uuid}, which is not in the package",
                    column.uuid
                )),
            }
        }
        // Coarsest to finest. A rank with no number keeps its place in the
        // list rather than being pushed to one end: `Ranks` is ordered and
        // that is the only thing known about it.
        ranks.sort_by_key(|rank| rank.number.unwrap_or(i64::MAX));
        out.columns.push(StratigraphicColumn { column, ranks, models: Vec::new() });
    }

    for object in &all {
        if object.type_info().type_key != "StratigraphicColumnRankInterpretation" {
            continue;
        }
        let uuid = identity::uuid_of_reflect(*object)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if listed.contains(&uuid) {
            continue;
        }
        out.notes.push(format!(
            "rank {uuid} is listed by no column - it is returned apart"
        ));
        out.loose_ranks.push(read_rank(*object, &held, &represents));
    }
    out.loose_ranks
        .sort_by_key(|rank| rank.number.unwrap_or(i64::MAX));
    attach_models(&mut out, &all, &held);
    out
}

/// The earth models of the package, attached to their columns and units.
///
/// One sweep over the `EarthModelInterpretation`s: each names its column, the
/// column's ranks list units, and every such unit gains the model — `root`
/// when it is the sole unit of the coarsest rank. A model whose column is not
/// in the package attaches to nothing, and says so.
fn attach_models(out: &mut Columns, all: &[&dyn Reflect], held: &Held<'_>) {
    for object in all {
        if object.type_info().type_key != "EarthModelInterpretation" {
            continue;
        }
        let emi = named(*object, held);
        let Some(FieldValue::Object(column)) = object.field("StratigraphicColumn") else {
            continue;
        };
        let column_uuid = identity::uuid_of_reflect(column).unwrap_or_default().to_ascii_lowercase();
        let model = match object.field("InterpretedFeature") {
            Some(FieldValue::Object(feature)) => named(feature, held),
            _ => Named::default(),
        };
        let listed = |name: &str| match object.field(name) {
            Some(FieldValue::List(list)) => !list.is_empty(),
            Some(FieldValue::Object(_)) => true,
            _ => false,
        };
        let created = match object.field("Citation") {
            Some(FieldValue::Object(citation)) => text(citation, "Creation").unwrap_or_default(),
            _ => String::new(),
        };
        let Some(target) = out.columns.iter_mut().find(|one| one.column.uuid == column_uuid) else {
            out.notes.push(format!(
                "earth model {} names column {column_uuid}, which is not in the package",
                emi.uuid
            ));
            continue;
        };
        target.models.push(emi.clone());
        // The coarsest rank is the first: `columns` sorts them so.
        let root = target.ranks.first().filter(|rank| rank.units.len() == 1).map(|rank| rank.units[0].unit.uuid.clone());
        let of = |unit: &str| ModelOf {
            emi: emi.clone(),
            model: model.clone(),
            root: root.as_deref() == Some(unit),
            created: created.clone(),
            structure: listed("Structure"),
            fluids: listed("Fluid"),
            wells: listed("WellboreInterpretationSet"),
        };
        let mut members: Vec<String> = target
            .ranks
            .iter()
            .flat_map(|rank| rank.units.iter().map(|unit| unit.unit.uuid.clone()))
            .collect();
        members.sort();
        members.dedup();
        // Every drawing of these units gets it — in the model's own column and
        // in any other column or loose rank that lists the same unit object.
        for column in out.columns.iter_mut() {
            for rank in column.ranks.iter_mut() {
                for unit in rank.units.iter_mut() {
                    if members.binary_search(&unit.unit.uuid).is_ok() {
                        unit.models.push(of(&unit.unit.uuid));
                    }
                }
            }
        }
        for rank in out.loose_ranks.iter_mut() {
            for unit in rank.units.iter_mut() {
                if members.binary_search(&unit.unit.uuid).is_ok() {
                    unit.models.push(of(&unit.unit.uuid));
                }
            }
        }
    }
    // The most recent snapshot first, on every card.
    let newest_first = |units: &mut Vec<ColumnUnit>| {
        for unit in units {
            unit.models.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.emi.uuid.cmp(&b.emi.uuid)));
        }
    };
    for column in out.columns.iter_mut() {
        for rank in column.ranks.iter_mut() {
            newest_first(&mut rank.units);
        }
    }
    for rank in out.loose_ranks.iter_mut() {
        newest_first(&mut rank.units);
    }
}

// ---------------------------------------------------------------------------
// A unit's subtree: what a model made from it contains
// ---------------------------------------------------------------------------

/// One rank of a subtree: the units of a source rank that lie inside the root.
#[derive(Clone, Debug, PartialEq)]
pub struct SubRank {
    /// The rank they come from.
    pub source: Named,
    /// Its number — kept by a restricted copy.
    pub number: Option<i64>,
    /// Its ordering criterion, verbatim.
    pub criteria: String,
    /// The units inside, in the source rank's order.
    pub units: Vec<Named>,
    /// **The units are exactly the source rank's**: the rank is taken as it is
    /// rather than copied.
    pub whole: bool,
    /// The source's contacts whose **two** sides are kept — statements of the
    /// file, copied as they are; a contact with one side outside is dropped.
    pub contacts: Vec<RankContact>,
}

/// What a model made from a unit contains, in one column.
#[derive(Clone, Debug, PartialEq)]
pub struct Subtree {
    /// The column the ranks come from; `None` for a rank no column lists.
    pub column: Option<Named>,
    /// The root's rank, then the finer ones, coarsest first.
    pub ranks: Vec<SubRank>,
    /// What could not be placed, and why — a rank or a unit, never guessed.
    pub unplaced: Vec<(Named, String)>,
}

/// One year, in millions of years: ages come from whole years, so two bounds
/// that agree to a year are one bound.
const AGE_TOLERANCE_MA: f64 = 1e-6;

/// The subtree of a unit, in each column that lists it
/// (the earth-models design).
///
/// The unit's rank is the **coarsest** that lists it; the finer ranks of the
/// same column are placed against it, each by the first rule that can answer:
///
/// 1. **a finer rank that lists the unit itself** does not subdivide it: the
///    unit is its own content there;
/// 2. **by age**, when the unit and every unit of the finer rank state one: a
///    unit is inside when its span is contained in the root's, to a year; a
///    unit that **overlaps** a limit is named in `unplaced`, never put on one
///    side;
/// 3. **by shared units** otherwise — the rule the front end's `alignRanks` uses: a
///    unit the two ranks share is a marker, and what lies between two markers
///    in the finer rank belongs to the root when the root is **alone** between
///    the same markers in its own rank;
/// 4. otherwise the rank is left out, **by its name**.
///
/// Refused when no rank lists the unit: nothing then says what it contains.
pub fn subtree(columns: &Columns, root: &str) -> Result<Vec<Subtree>, String> {
    let root = root.to_ascii_lowercase();
    let mut out = Vec::new();
    for column in &columns.columns {
        if let Some(tree) = subtree_in(Some(&column.column), &column.ranks, &root) {
            out.push(tree);
        }
    }
    for rank in &columns.loose_ranks {
        if let Some(tree) = subtree_in(None, std::slice::from_ref(rank), &root) {
            out.push(tree);
        }
    }
    if out.is_empty() {
        return Err(format!("unit {root} is listed by no rank — nothing says what it contains"));
    }
    Ok(out)
}

/// A whole column as a subtree: every rank taken as it is. What a model of a
/// whole column contains.
pub fn whole_column(column: &StratigraphicColumn) -> Subtree {
    Subtree {
        column: Some(column.column.clone()),
        ranks: column
            .ranks
            .iter()
            .map(|rank| SubRank {
                source: rank.rank.clone(),
                number: rank.number,
                criteria: rank.criteria.clone(),
                units: rank.units.iter().map(|unit| unit.unit.clone()).collect(),
                whole: true,
                contacts: rank.contacts.clone(),
            })
            .collect(),
        unplaced: Vec::new(),
    }
}

fn subtree_in(column: Option<&Named>, ranks: &[ColumnRank], root: &str) -> Option<Subtree> {
    let at = ranks.iter().position(|rank| rank.units.iter().any(|unit| unit.unit.uuid == root))?;
    let home = &ranks[at];
    let root_unit = home.units.iter().find(|unit| unit.unit.uuid == root)?;
    let mut tree = Subtree { column: column.cloned(), ranks: Vec::new(), unplaced: Vec::new() };
    tree.ranks.push(restricted(home, &[root.to_string()]));

    for finer in &ranks[at + 1..] {
        // 1. Not subdivided at this rank.
        if finer.units.iter().any(|unit| unit.unit.uuid == root) {
            tree.ranks.push(restricted(finer, &[root.to_string()]));
            continue;
        }
        // 2. By age.
        if let Some([old, young]) = root_unit.age
            && finer.units.iter().all(|unit| unit.age.is_some())
        {
            let mut inside = Vec::new();
            for unit in &finer.units {
                let [u_old, u_young] = unit.age.expect("checked above");
                let contained = u_old <= old + AGE_TOLERANCE_MA && u_young >= young - AGE_TOLERANCE_MA;
                let apart = u_young >= old - AGE_TOLERANCE_MA || u_old <= young + AGE_TOLERANCE_MA;
                if contained {
                    inside.push(unit.unit.uuid.clone());
                } else if !apart {
                    tree.unplaced.push((
                        unit.unit.clone(),
                        format!(
                            "{} ({u_old}–{u_young} Ma) overlaps a limit of {} ({old}–{young} Ma) — left out rather than put on one side",
                            label(&unit.unit),
                            label(&root_unit.unit)
                        ),
                    ));
                }
            }
            if !inside.is_empty() {
                tree.ranks.push(restricted(finer, &inside));
            }
            continue;
        }
        // 3. By the units the two ranks share.
        match between_markers(home, finer, root) {
            Ok(inside) if !inside.is_empty() => tree.ranks.push(restricted(finer, &inside)),
            Ok(_) => {}
            Err(why) => tree.unplaced.push((finer.rank.clone(), why)),
        }
    }
    Some(tree)
}

/// Rule 3: the units of `finer` between the markers that bound `root` in `home`.
fn between_markers(home: &ColumnRank, finer: &ColumnRank, root: &str) -> Result<Vec<String>, String> {
    let in_finer: HashMap<&str, usize> =
        finer.units.iter().enumerate().map(|(at, unit)| (unit.unit.uuid.as_str(), at)).collect();
    let here = home.units.iter().position(|unit| unit.unit.uuid == root).expect("the root is in its rank");
    let above = home.units[..here].iter().rposition(|unit| in_finer.contains_key(unit.unit.uuid.as_str()));
    let below = home.units[here + 1..]
        .iter()
        .position(|unit| in_finer.contains_key(unit.unit.uuid.as_str()))
        .map(|offset| here + 1 + offset);
    if above.is_none() && below.is_none() {
        return Err(format!(
            "rank {} states no age and shares no unit with {} — left out of the model rather than guessed",
            label(&finer.rank),
            label(&home.rank)
        ));
    }
    // The root must be alone between the markers in its own rank: otherwise
    // what lies between them in the finer rank belongs to several units, and
    // nothing says which.
    let from = above.map_or(0, |at| at + 1);
    let to = below.unwrap_or(home.units.len());
    if to - from != 1 {
        return Err(format!(
            "rank {}: {} is not alone between the units it shares with {} — which of them owns the units between is not stated",
            label(&finer.rank),
            label(&home.units[here].unit),
            label(&home.rank)
        ));
    }
    let start = above.map_or(0, |at| in_finer[home.units[at].unit.uuid.as_str()] + 1);
    let end = below.map_or(finer.units.len(), |at| in_finer[home.units[at].unit.uuid.as_str()]);
    Ok(finer.units.get(start..end).unwrap_or_default().iter().map(|unit| unit.unit.uuid.clone()).collect())
}

/// A rank restricted to some of its units, in its own order.
fn restricted(rank: &ColumnRank, keep: &[String]) -> SubRank {
    let units: Vec<Named> =
        rank.units.iter().filter(|unit| keep.contains(&unit.unit.uuid)).map(|unit| unit.unit.clone()).collect();
    let kept = |uuid: &str| units.iter().any(|unit| unit.uuid == uuid);
    SubRank {
        source: rank.rank.clone(),
        number: rank.number,
        criteria: rank.criteria.clone(),
        whole: units.len() == rank.units.len(),
        contacts: rank
            .contacts
            .iter()
            .filter(|contact| kept(&contact.subject.uuid) && kept(&contact.object.uuid))
            .cloned()
            .collect(),
        units,
    }
}

/// A title when there is one, the uuid otherwise.
fn label(named: &Named) -> String {
    if named.title.is_empty() { named.uuid.clone() } else { format!("`{}`", named.title) }
}

// ---------------------------------------------------------------------------
// Structural organisations: horizons, faults, and what cuts what
// ---------------------------------------------------------------------------

/// The 2.0.1 list of an organisation's faults. 2.2 has none: its faults are
/// entries of the same ordered list as its horizons, told apart by type.
const FAULTS: &str = "Faults";
/// The frontiers of an organisation's volume of interest.
const FRONTIERS: &[&str] = &["TopFrontier", "BottomFrontier", "Sides"];
/// The verbs by which a fault **offsets** a surface — and so is younger than
/// it. `stops` is not among them: a fault that stops at a horizon may be
/// sealed by it or rooted in it, and nothing in the contact says which.
const OFFSETS: &[&str] = &["splits", "crosses"];

/// A horizon of an organisation, in its place.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    /// The horizon.
    pub surface: Named,
    /// The stratigraphic rank its entry declares.
    pub rank: Option<i64>,
    /// `IsConformableAbove` / `IsConformableBelow` (2.2) — `None` when the
    /// file says nothing, which is not *conformable*.
    pub conformable_above: Option<bool>,
    /// See `conformable_above`.
    pub conformable_below: Option<bool>,
    /// `BoundaryRelation` (2.0.1, and an open list in 2.2), verbatim.
    pub relations: Vec<String>,
}

/// A fault of an organisation, with what the file says of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Fault {
    /// The fault interpretation.
    pub fault: Named,
    /// The kinds of throw its `ThrowInterpretation`s state — `normal`,
    /// `reverse`, `dextral`, … verbatim, each once.
    pub throws: Vec<String>,
    /// `MaximumThrow`, `MeanDip`, `MeanAzimuth`, as `value uom`.
    pub maximum_throw: Option<String>,
    /// `MeanDip`.
    pub dip: Option<String>,
    /// `MeanAzimuth`.
    pub azimuth: Option<String>,
    /// `IsListric`, and `IsSealed` (2.2 only).
    pub listric: Option<bool>,
    /// `IsSealed`.
    pub sealed: Option<bool>,
    /// When it moved, `[old, young]` in Ma, when a throw or the fault states
    /// it as numbers — the same reading as a unit's age.
    pub active: Option<[f64; 2]>,
    /// The horizons it **offsets** (`splits`, `crosses`), uuids in the
    /// organisation's order. Cross-cutting: the fault is younger than each.
    pub offsets: Vec<String>,
}

/// A structural organisation, as the view draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Organisation {
    /// The organisation itself.
    pub organisation: Named,
    /// The ordering criterion, verbatim.
    pub criteria: String,
    /// The horizons, **top down** when the criterion descends — the list's
    /// order otherwise, and `descends` says which.
    pub horizons: Vec<Surface>,
    /// Whether the criterion is one the schema defines, all three of which
    /// run from the top down.
    pub descends: bool,
    /// The faults, in the order the file lists them — an order that means
    /// nothing for a fault, and is not used as one.
    pub faults: Vec<Fault>,
    /// The frontiers of the volume: top, bottom, sides.
    pub frontiers: Vec<Named>,
    /// Every contact, as the file writes it.
    pub contacts: Vec<RankContact>,
    /// What could not be read, named.
    pub notes: Vec<String>,
}

/// A measure as `value uom`, whatever the spelling of its two fields.
fn measure(object: &dyn Reflect, name: &str) -> Option<String> {
    let FieldValue::Object(measure) = object.field(name)? else { return None };
    let value = ["Value", "value"].iter().find_map(|field| match measure.field(field) {
        Some(FieldValue::Scalar(scalar)) => Some(scalar.to_string()),
        _ => None,
    })?;
    let uom = ["uom", "Uom"].iter().find_map(|field| match measure.field(field) {
        Some(FieldValue::Scalar(scalar)) => Some(scalar.to_string()),
        _ => None,
    });
    Some(match uom {
        Some(uom) => format!("{value} {uom}"),
        None => value,
    })
}

/// A boolean field, or `None`: absent is not false.
fn flag(object: &dyn Reflect, name: &str) -> Option<bool> {
    match object.field(name)? {
        FieldValue::Scalar(scalar) => match scalar.to_string().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Read a fault interpretation — the object when the package holds it, the
/// reference alone otherwise.
fn read_fault(reference: Named, held: &Held<'_>) -> Fault {
    let object = held.get(&reference.uuid).copied();
    let mut throws: Vec<String> = Vec::new();
    let mut active = None;
    if let Some(object) = object {
        if let Some(FieldValue::List(parts)) = object.field("ThrowInterpretation") {
            for part in parts {
                let FieldValue::Object(part) = part else { continue };
                for kind in scalars(part, "Throw") {
                    if !throws.contains(&kind) {
                        throws.push(kind);
                    }
                }
                active = active.or_else(|| age_of(part));
            }
        }
        active = active.or_else(|| age_of(object));
    }
    Fault {
        throws,
        maximum_throw: object.and_then(|o| measure(o, "MaximumThrow")),
        dip: object.and_then(|o| measure(o, "MeanDip")),
        azimuth: object.and_then(|o| measure(o, "MeanAzimuth")),
        listric: object.and_then(|o| flag(o, "IsListric")),
        sealed: object.and_then(|o| flag(o, "IsSealed")),
        active,
        offsets: Vec::new(),
        fault: reference,
    }
}

/// Read one `StructuralOrganizationInterpretation`.
fn read_organisation(object: &dyn Reflect, held: &Held<'_>) -> Organisation {
    let mut notes = Vec::new();
    let organisation = named(object, held);
    let criteria = text_any(object, CRITERIA).unwrap_or_default();
    let descends = matches!(criteria.trim(), "age" | "apparent depth" | "measured depth");
    if !descends {
        notes.push(format!(
            "organisation {} states \"{}\" as its criterion — the direction of its horizons is not asserted",
            organisation.title, criteria
        ));
    }

    let mut horizons: Vec<(i64, Surface)> = Vec::new();
    let mut faults: Vec<Fault> = Vec::new();
    let entries = match field_any(object, ORDERED) {
        Some(FieldValue::List(entries)) => entries,
        _ => Vec::new(),
    };
    for (position, entry) in entries.iter().enumerate() {
        let FieldValue::Object(entry) = entry else { continue };
        let Some(FieldValue::Object(reference)) = field_any(*entry, ENTRY_REF) else {
            // A `FeatureInterpretationSet` in place of a single surface: said,
            // not guessed at.
            notes.push(format!("entry {position} of the ordered list is not a single surface — left out"));
            continue;
        };
        let surface = named(reference, held);
        // The type decides, not the list: 2.2 puts its faults in the same
        // ordered list as its horizons, where their place means nothing.
        if surface.kind.contains("Fault") {
            if !faults.iter().any(|f| f.fault.uuid == surface.uuid) {
                faults.push(read_fault(surface, held));
            }
            continue;
        }
        let target = held.get(&surface.uuid).copied();
        horizons.push((
            integer(*entry, INDEX).unwrap_or(position as i64),
            Surface {
                rank: integer(*entry, RANK),
                conformable_above: target.and_then(|t| flag(t, "IsConformableAbove")),
                conformable_below: target.and_then(|t| flag(t, "IsConformableBelow")),
                relations: target.map(|t| scalars(t, BOUNDARY)).unwrap_or_default(),
                surface,
            },
        ));
    }
    horizons.sort_by_key(|(index, _)| *index);
    let horizons: Vec<Surface> = horizons.into_iter().map(|(_, surface)| surface).collect();

    if let Some(FieldValue::List(references)) = object.field(FAULTS) {
        for reference in references {
            let FieldValue::Object(reference) = reference else { continue };
            let fault = named(reference, held);
            if !faults.iter().any(|f| f.fault.uuid == fault.uuid) {
                faults.push(read_fault(fault, held));
            }
        }
    }

    let mut frontiers = Vec::new();
    for name in FRONTIERS {
        let references = match object.field(name) {
            Some(FieldValue::List(references)) => references,
            Some(other) => vec![other],
            None => Vec::new(),
        };
        for reference in references {
            if let FieldValue::Object(reference) = reference {
                frontiers.push(named(reference, held));
            }
        }
    }

    let mut contacts = contacts_of(object, held);
    let position: HashMap<String, usize> =
        horizons.iter().enumerate().map(|(at, h)| (h.surface.uuid.clone(), at)).collect();
    read_unconformities(&mut contacts, descends.then_some(&position), held);

    // Cross-cutting: a fault that splits or crosses a horizon is younger than
    // it. The fault is looked for on **either** side of the contact — the
    // reference package writes it as the subject, and nothing obliges that.
    for fault in &mut faults {
        let mut offsets: Vec<String> = Vec::new();
        for contact in &contacts {
            if !OFFSETS.contains(&contact.verb.trim()) {
                continue;
            }
            let other = if contact.subject.uuid == fault.fault.uuid {
                &contact.object
            } else if contact.object.uuid == fault.fault.uuid {
                &contact.subject
            } else {
                continue;
            };
            if position.contains_key(&other.uuid) && !offsets.contains(&other.uuid) {
                offsets.push(other.uuid.clone());
            }
        }
        offsets.sort_by_key(|uuid| position.get(uuid).copied().unwrap_or(usize::MAX));
        fault.offsets = offsets;
    }

    Organisation { organisation, criteria, horizons, descends, faults, frontiers, contacts, notes }
}

/// Every structural organisation of a package.
///
/// Several are ordinary — two structural scenarios of one model — and none is
/// preferred: the view offers a choice (with several structural
/// organisations, a selector).
pub fn structures<'a>(objects: impl Iterator<Item = &'a dyn Reflect>) -> Vec<Organisation> {
    let all: Vec<&dyn Reflect> = objects.collect();
    let held: Held<'_> = all
        .iter()
        .filter_map(|object| {
            identity::uuid_of_reflect(*object)
                .filter(|uuid| !uuid.is_empty())
                .map(|uuid| (uuid.to_ascii_lowercase(), *object))
        })
        .collect();
    all.iter()
        .filter(|object| object.type_info().type_key == "StructuralOrganizationInterpretation")
        .map(|object| read_organisation(*object, &held))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use energyml_resqml_2_0_1 as resqml20;

    fn reference(kind: &str, uuid: &str) -> energyml_eml_2_0::DataObjectReference {
        energyml_eml_2_0::DataObjectReference {
            content_type: format!("application/x-resqml+xml;version=2.0;type=obj_{kind}"),
            title: "target".into(),
            uuid: uuid.into(),
            ..Default::default()
        }
    }

    fn contact_element(uuid: &str) -> resqml20::ContactElementReference {
        resqml20::ContactElementReference {
            content_type: "application/x-resqml+xml;version=2.0;type=obj_HorizonInterpretation".into(),
            title: "target".into(),
            uuid: uuid.into(),
            ..Default::default()
        }
    }

    fn organisation(uuid: &str, horizons: &[(u64, &str)]) -> resqml20::StructuralOrganizationInterpretation {
        resqml20::StructuralOrganizationInterpretation {
            uuid: uuid.into(),
            ordering_criteria: resqml20::OrderingCriteria::Age,
            horizons: horizons
                .iter()
                .map(|(index, target)| resqml20::HorizonInterpretationIndex {
                    index: *index,
                    stratigraphic_rank: Some(1),
                    horizon: reference("HorizonInterpretation", target),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn the_order_comes_from_the_organisation_with_its_criterion() {
        let org = organisation(
            "00000000-0000-0000-0000-0000000000aa",
            &[(0, "h-top"), (1, "h-bottom")],
        );
        let order = read([&org as &dyn Reflect].into_iter());
        assert_eq!(order.len(), 2);
        let top = order.of_interpretation("h-top").expect("placed");
        assert_eq!(top.index, 0);
        assert_eq!(top.rank, Some(1));
        // The provenance travels with the order: without it, a wrong order
        // cannot be disputed.
        assert_eq!(top.organisation, "00000000-0000-0000-0000-0000000000aa");
        assert_eq!(top.criteria, "age");
        // The schema's three criteria all descend.
        assert!(top.descends());
        assert!(
            order.of_interpretation("H-TOP").is_some(),
            "a uuid is written in both cases"
        );
    }

    #[test]
    fn an_unknown_criterion_is_kept_but_not_asserted_to_descend() {
        // A value from a future version, or an `Other` from an open
        // enumeration: the order is read, but its **direction** is not invented.
        let mut org = organisation("00000000-0000-0000-0000-0000000000bb", &[(0, "h-1")]);
        org.ordering_criteria = resqml20::OrderingCriteria::MeasuredDepth;
        let order = read([&org as &dyn Reflect].into_iter());
        assert!(order.of_interpretation("h-1").expect("placed").descends());

        let odd = Placed {
            index: 0,
            rank: None,
            organisation: "x".into(),
            criteria: "quelque chose d'autre".into(),
        };
        assert!(!odd.descends());
    }

    #[test]
    fn two_organisations_that_disagree_are_reported_never_arbitrated() {
        // Silently preferring one would mean choosing between two structural
        // models without saying so.
        let first = organisation("00000000-0000-0000-0000-0000000000cc", &[(0, "h-1")]);
        let second = organisation("00000000-0000-0000-0000-0000000000dd", &[(5, "h-1")]);
        let order = read([&first as &dyn Reflect, &second as &dyn Reflect].into_iter());
        assert_eq!(order.of_interpretation("h-1").expect("placed").index, 0);
        assert_eq!(order.notes.len(), 1);
        assert!(order.notes[0].contains("two organisations"), "{:?}", order.notes);
    }

    #[test]
    fn the_package_can_explain_a_crossing_and_saying_nothing_is_an_answer() {
        // The rule of section 5: read, measure, confront. Here the reading -
        // a contact "X erodes Y" with its mode, which a crossed geometry will
        // be able to confront instead of being guessed.
        let org = resqml20::StructuralOrganizationInterpretation {
            uuid: "00000000-0000-0000-0000-00000000c001".into(),
            ordering_criteria: resqml20::OrderingCriteria::Age,
            contact_interpretation: vec![
                resqml20::AbstractContactInterpretationPart::BinaryContactInterpretationPart(
                    resqml20::BinaryContactInterpretationPart {
                        index: 0,
                        verb: resqml20::ContactVerb::Erodes,
                        subject: contact_element("h-young"),
                        direct_object: contact_element("h-old"),
                        ..Default::default()
                    },
                ),
            ],
            ..Default::default()
        };
        let said = statements([&org as &dyn Reflect].into_iter());
        assert_eq!(said.contacts().len(), 1);

        let explained = said.explains("h-young", "h-old");
        assert_eq!(explained.len(), 1, "{explained:?}");
        assert!(explained[0].contains("erodes"), "{explained:?}");
        // The pair is recognised both ways round: a contact is a relation,
        // not a reading direction.
        assert_eq!(said.explains("h-old", "h-young").len(), 1);

        // A pair the package says nothing about: **empty**, and that is the
        // answer that calls for a human rather than an invented erosion.
        assert!(said.explains("h-young", "h-autre").is_empty());
    }

    #[test]
    fn a_boundary_relation_that_says_unconformable_explains_too() {
        let horizon = resqml20::HorizonInterpretation {
            uuid: "00000000-0000-0000-0000-00000000b001".into(),
            boundary_relation: vec![resqml20::BoundaryRelation::UnconformableAbove],
            ..Default::default()
        };
        let said = statements([&horizon as &dyn Reflect].into_iter());
        let explained = said.explains("00000000-0000-0000-0000-00000000b001", "autre");
        assert_eq!(explained.len(), 1, "{explained:?}");
        assert!(explained[0].contains("unconformable"), "{explained:?}");

        // `conformable` does **not** explain a crossing: it is the very
        // opposite, and counting it would explain everything away.
        let plain = resqml20::HorizonInterpretation {
            uuid: "00000000-0000-0000-0000-00000000b002".into(),
            boundary_relation: vec![resqml20::BoundaryRelation::Conformable],
            ..Default::default()
        };
        let said = statements([&plain as &dyn Reflect].into_iter());
        assert!(
            said.explains("00000000-0000-0000-0000-00000000b002", "autre")
                .is_empty()
        );
    }

    #[test]
    fn a_representation_names_the_interpretation_it_represents() {
        let surface = resqml20::TriangulatedSetRepresentation {
            uuid: "00000000-0000-0000-0000-0000000000ee".into(),
            represented_interpretation: Some(reference("HorizonInterpretation", "h-1")),
            ..Default::default()
        };
        assert_eq!(interpretation_of(&surface).as_deref(), Some("h-1"));

        // A representation that represents nothing is an answer, not a
        // failure: the section will classify it by hand.
        let orphan = resqml20::TriangulatedSetRepresentation::default();
        assert_eq!(interpretation_of(&orphan), None);
    }

    #[test]
    fn an_organisation_with_no_ordered_list_is_noted_rather_than_skipped() {
        let empty = resqml20::StructuralOrganizationInterpretation {
            uuid: "00000000-0000-0000-0000-0000000000ff".into(),
            ordering_criteria: resqml20::OrderingCriteria::Age,
            ..Default::default()
        };
        let order = read([&empty as &dyn Reflect].into_iter());
        assert!(order.is_empty());
        assert_eq!(order.notes.len(), 1);
        assert!(order.notes[0].contains("no ordered horizon"), "{:?}", order.notes);
    }

    // -----------------------------------------------------------------------
    // The column
    // -----------------------------------------------------------------------

    /// A 2.0.1 column whose document lists its units **out of order**.
    ///
    /// The case the corpus does not hold and that must work all the same: in
    /// 2.0.1 it is the declared `Index` that orders, not the position. A reader
    /// taking the document would return the reverse here - and an inverted
    /// stack looks exactly like a correct one.
    #[test]
    fn the_declared_index_orders_not_the_document() {
        let entry = |index: u64, uuid: &str| resqml20::StratigraphicUnitInterpretationIndex {
            index,
            unit: reference("StratigraphicUnitInterpretation", uuid),
        };
        let rank = resqml20::StratigraphicColumnRankInterpretation {
            uuid: "00000000-0000-0000-0000-0000000000a1".into(),
            ordering_criteria: resqml20::OrderingCriteria::Age,
            index: 0,
            // The bottom written before the top, which nothing forbids.
            stratigraphic_units: vec![entry(1, "u-bottom"), entry(0, "u-top")],
            ..Default::default()
        };
        let all = columns([&rank as &dyn Reflect].into_iter());
        // No column lists it, so it is returned apart - and named.
        assert!(all.columns.is_empty());
        assert_eq!(all.loose_ranks.len(), 1);
        assert!(
            all.notes.iter().any(|n| n.contains("no column")),
            "{:?}",
            all.notes
        );

        let read = &all.loose_ranks[0];
        assert_eq!(
            read.units
                .iter()
                .map(|u| u.unit.uuid.as_str())
                .collect::<Vec<_>>(),
            ["u-top", "u-bottom"],
            "the declared index orders, not the position in the document"
        );
        assert_eq!(read.number, Some(0));
        assert!(
            read.descends(),
            "`age` is one of the three criteria that descend"
        );
        // The units are not in the package: drawn from the reference alone,
        // and saying so stops an empty title passing for an object with no
        // title.
        assert!(read.units.iter().all(|u| !u.resolved));
        assert_eq!(
            read.units.iter().filter(|u| !u.resolved).count(),
            read.notes.len()
        );
    }

    /// A complete column: the ranks come with it, and a representation finds its
    /// unit **against the current** of the reference.
    #[test]
    fn a_column_carries_its_ranks_and_each_unit_what_represents_it() {
        let unit = |uuid: &str, title: &str| resqml20::StratigraphicUnitInterpretation {
            uuid: uuid.into(),
            citation: energyml_eml_2_0::Citation {
                title: title.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let top = unit("u-top", "Le toit");
        let bottom = unit("u-bottom", "La base");
        let rank = resqml20::StratigraphicColumnRankInterpretation {
            uuid: "k-0".into(),
            ordering_criteria: resqml20::OrderingCriteria::ApparentDepth,
            index: 0,
            stratigraphic_units: vec![
                resqml20::StratigraphicUnitInterpretationIndex {
                    index: 0,
                    unit: reference("StratigraphicUnitInterpretation", "u-top"),
                },
                resqml20::StratigraphicUnitInterpretationIndex {
                    index: 1,
                    unit: reference("StratigraphicUnitInterpretation", "u-bottom"),
                },
            ],
            ..Default::default()
        };
        let column = resqml20::StratigraphicColumn {
            uuid: "c-0".into(),
            citation: energyml_eml_2_0::Citation {
                title: "La colonne".into(),
                ..Default::default()
            },
            ranks: vec![reference("StratigraphicColumnRankInterpretation", "k-0")],
            ..Default::default()
        };
        let surface = resqml20::TriangulatedSetRepresentation {
            uuid: "r-0".into(),
            citation: energyml_eml_2_0::Citation {
                title: "Le toit, triangulé".into(),
                ..Default::default()
            },
            represented_interpretation: Some(reference("StratigraphicUnitInterpretation", "u-top")),
            ..Default::default()
        };

        let all = columns(
            [
                &column as &dyn Reflect,
                &rank as &dyn Reflect,
                &top as &dyn Reflect,
                &bottom as &dyn Reflect,
                &surface as &dyn Reflect,
            ]
            .into_iter(),
        );
        assert_eq!(all.columns.len(), 1);
        assert!(all.loose_ranks.is_empty(), "the rank is listed, so not apart");
        let read = &all.columns[0];
        assert_eq!(read.column.title, "La colonne");
        assert_eq!(read.ranks.len(), 1);

        let units = &read.ranks[0].units;
        // The title comes from the **object**, not from the reference's cache
        // - which says "target" here and which, in a real package, may be stale.
        assert_eq!(units[0].unit.title, "Le toit");
        assert_eq!(units[0].unit.kind, "StratigraphicUnitInterpretation");
        assert!(units[0].resolved);
        // The reference runs from the representation to the unit: following it
        // the direct way would never find anything.
        assert_eq!(
            units[0]
                .represented
                .iter()
                .map(|r| r.title.as_str())
                .collect::<Vec<_>>(),
            ["Le toit, triangulé"]
        );
        assert!(units[1].represented.is_empty(), "nothing represents the base");
    }

    /// Where a column draws an erosion: the sides of a contact the file says
    /// are not conformable, each by the unit on that side.
    ///
    /// RESQML 2.0.1: `ContactMode` `erosion` on a side (the unit is truncated),
    /// and the `BoundaryRelation` of the horizon the contact is `PartOf` —
    /// *"unconformable below"* speaks of what is under the horizon, turned
    /// into a unit by the rank's order.
    #[test]
    fn an_unconformity_is_read_from_the_mode_and_from_the_horizon() {
        let entry = |index: u64, uuid: &str| resqml20::StratigraphicUnitInterpretationIndex {
            index,
            unit: reference("StratigraphicUnitInterpretation", uuid),
        };
        let side = |uuid: &str, mode: Option<resqml20::ContactMode>| resqml20::ContactElementReference {
            content_type: "application/x-resqml+xml;version=2.0;type=obj_StratigraphicUnitInterpretation".into(),
            title: uuid.into(),
            uuid: uuid.into(),
            secondary_qualifier: mode,
            ..Default::default()
        };
        let contact = |subject, object, horizon: &str| {
            resqml20::AbstractContactInterpretationPart::BinaryContactInterpretationPart(
                resqml20::BinaryContactInterpretationPart {
                    subject,
                    direct_object: object,
                    part_of: Some(reference("HorizonInterpretation", horizon)),
                    ..Default::default()
                },
            )
        };
        let rank = resqml20::StratigraphicColumnRankInterpretation {
            uuid: "k-e".into(),
            ordering_criteria: resqml20::OrderingCriteria::Age,
            stratigraphic_units: vec![entry(0, "u-a"), entry(1, "u-b"), entry(2, "u-c")],
            contact_interpretation: vec![
                // Written bottom-up on purpose: the direction is not data.
                contact(side("u-b", None), side("u-a", None), "h-ab"),
                contact(side("u-c", Some(resqml20::ContactMode::Erosion)), side("u-b", None), "h-bc"),
            ],
            ..Default::default()
        };
        let unconformable = resqml20::HorizonInterpretation {
            uuid: "h-ab".into(),
            citation: energyml_eml_2_0::Citation { title: "H1".into(), ..Default::default() },
            boundary_relation: vec![resqml20::BoundaryRelation::UnconformableBelow],
            ..Default::default()
        };
        let conformable = resqml20::HorizonInterpretation {
            uuid: "h-bc".into(),
            boundary_relation: vec![resqml20::BoundaryRelation::Conformable],
            ..Default::default()
        };
        let all = columns(
            [&rank as &dyn Reflect, &unconformable as &dyn Reflect, &conformable as &dyn Reflect].into_iter(),
        );
        let read = &all.loose_ranks[0];
        let sides = |at: usize| {
            read.contacts[at].unconformable.iter().map(|side| side.unit.as_str()).collect::<Vec<_>>()
        };
        // "unconformable below" H1: the unit under it, u-b — though the
        // contact names u-b as its subject. A side names its unit by full uri.
        assert_eq!(sides(0), ["eml:///resqml20.obj_StratigraphicUnitInterpretation(u-b)"]);
        assert!(read.contacts[0].unconformable[0].says.contains("H1"), "{:?}", read.contacts[0]);
        // `erosion` on u-c's side; the conformable horizon adds nothing.
        assert_eq!(sides(1), ["eml:///resqml20.obj_StratigraphicUnitInterpretation(u-c)"]);
        assert!(read.contacts[1].unconformable[0].says.contains("erosion"));
    }

    /// RESQML 2.0.1: the organisation lists its faults apart, its horizons by
    /// index, and says what cuts what in its contacts. A fault that `splits`
    /// two horizons is younger than both — cross-cutting, the only relative
    /// dating the contacts support — and the list comes back in the
    /// organisation's order, top first, whatever the document order.
    #[test]
    fn an_organisation_reads_its_horizons_its_faults_and_what_cuts_what() {
        let fault_ref = || reference("FaultInterpretation", "f-1");
        let side = |kind: &str, uuid: &str| resqml20::ContactElementReference {
            content_type: format!("application/x-resqml+xml;version=2.0;type=obj_{kind}"),
            title: uuid.into(),
            uuid: uuid.into(),
            ..Default::default()
        };
        let splits = |horizon: &str| {
            resqml20::AbstractContactInterpretationPart::BinaryContactInterpretationPart(
                resqml20::BinaryContactInterpretationPart {
                    subject: side("FaultInterpretation", "f-1"),
                    verb: resqml20::ContactVerb::Splits,
                    direct_object: side("HorizonInterpretation", horizon),
                    ..Default::default()
                },
            )
        };
        let organisation = resqml20::StructuralOrganizationInterpretation {
            uuid: "o-1".into(),
            ordering_criteria: resqml20::OrderingCriteria::Age,
            horizons: vec![
                // Written bottom first: the index orders, not the document.
                resqml20::HorizonInterpretationIndex {
                    index: 1,
                    stratigraphic_rank: None,
                    horizon: reference("HorizonInterpretation", "h-base"),
                },
                resqml20::HorizonInterpretationIndex {
                    index: 0,
                    stratigraphic_rank: None,
                    horizon: reference("HorizonInterpretation", "h-top"),
                },
            ],
            faults: vec![fault_ref()],
            contact_interpretation: vec![splits("h-base"), splits("h-top")],
            ..Default::default()
        };
        let fault = resqml20::FaultInterpretation {
            uuid: "f-1".into(),
            citation: energyml_eml_2_0::Citation { title: "F1".into(), ..Default::default() },
            throw_interpretation: vec![resqml20::FaultThrow {
                throw: vec![resqml20::ThrowKind::Normal],
                ..Default::default()
            }],
            ..Default::default()
        };
        let read = structures([&organisation as &dyn Reflect, &fault as &dyn Reflect].into_iter());
        assert_eq!(read.len(), 1);
        let one = &read[0];
        assert!(one.descends);
        assert_eq!(
            one.horizons.iter().map(|h| h.surface.uuid.as_str()).collect::<Vec<_>>(),
            ["h-top", "h-base"]
        );
        assert_eq!(one.faults.len(), 1);
        let f = &one.faults[0];
        assert_eq!(f.fault.title, "F1", "the title comes from the object");
        assert_eq!(f.throws, ["normal"]);
        // In the organisation's order, top first — the contacts listed it last.
        assert_eq!(f.offsets, ["h-top", "h-base"]);
    }

    /// RESQML 2.2: the fault is an entry of the **same** ordered list as the
    /// horizons — first, in the reference package — and must not be read as
    /// a horizon above them. And a horizon's `IsConformableBelow = false`
    /// comes through.
    #[test]
    fn a_22_fault_in_the_ordered_list_is_a_fault_not_a_horizon() {
        use energyml_resqml_2_2 as resqml22;
        let dor = |kind: &str, uuid: &str| energyml_eml_2_3::DataObjectReference {
            uuid: uuid.into(),
            qualified_type: format!("resqml22.{kind}"),
            title: uuid.into(),
            ..Default::default()
        };
        let entry = |kind: &str, uuid: &str| resqml22::BoundaryFeatureInterpretationPlusItsRank {
            stratigraphic_rank: None,
            boundary_feature_interpretation: Some(dor(kind, uuid)),
            feature_interpretation_set: None,
        };
        let organisation = resqml22::StructuralOrganizationInterpretation {
            uuid: "o-2".into(),
            ascending_ordering_criteria: resqml22::OrderingCriteria::ApparentDepth,
            ordered_boundary_feature_interpretation: vec![
                entry("FaultInterpretation", "f-2"),
                entry("HorizonInterpretation", "h-1"),
                entry("HorizonInterpretation", "h-2"),
            ],
            ..Default::default()
        };
        let horizon = resqml22::HorizonInterpretation {
            uuid: "h-1".into(),
            is_conformable_below: Some(false),
            ..Default::default()
        };
        let read = structures([&organisation as &dyn Reflect, &horizon as &dyn Reflect].into_iter());
        let one = &read[0];
        assert_eq!(
            one.horizons.iter().map(|h| h.surface.uuid.as_str()).collect::<Vec<_>>(),
            ["h-1", "h-2"]
        );
        assert_eq!(one.faults.iter().map(|f| f.fault.uuid.as_str()).collect::<Vec<_>>(), ["f-2"]);
        assert_eq!(one.horizons[0].conformable_below, Some(false));
        assert_eq!(one.horizons[1].conformable_below, None, "not said is not conformable");
    }

    /// Two entries carrying the same index are a fault of the file, and it is
    /// said rather than settled in silence.
    #[test]
    fn two_units_sharing_an_index_are_reported() {
        let entry = |index: u64, uuid: &str| resqml20::StratigraphicUnitInterpretationIndex {
            index,
            unit: reference("StratigraphicUnitInterpretation", uuid),
        };
        let rank = resqml20::StratigraphicColumnRankInterpretation {
            uuid: "k-dup".into(),
            ordering_criteria: resqml20::OrderingCriteria::Age,
            stratigraphic_units: vec![entry(0, "u-1"), entry(0, "u-2")],
            ..Default::default()
        };
        let all = columns([&rank as &dyn Reflect].into_iter());
        let read = &all.loose_ranks[0];
        assert!(
            read.notes.iter().any(|n| n.contains("index 0")),
            "{:?}",
            read.notes
        );
        // Settled by the document order, which is stable and is stated.
        assert_eq!(
            read.units
                .iter()
                .map(|u| u.unit.uuid.as_str())
                .collect::<Vec<_>>(),
            ["u-1", "u-2"]
        );
    }
}
