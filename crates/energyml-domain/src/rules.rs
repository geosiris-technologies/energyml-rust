//! Business constraints - the engine of ADR-0014
//! (increments 1 to 4: BRT1, BRT2, BRT9, then BRT4 and BRT5, then the BRT3
//! propagation, then the TRG's own set - see [`trg`]).
//!
//! A rule is **data** (JSON in a registry), never code - the pattern of the
//! metadata specs. The engine is a free function over objects already in
//! memory and an [`ObjectSource`], the lesson of `graph.rs`: no store, no I/O,
//! no synchronous/asynchronous colour - the ETP client will get it for
//! nothing.
//!
//! The vocabulary is closed and narrow: `targets` (a reference aims at a
//! concept - BRT1, BRT9), `equals` (a field holds - the `when` condition and
//! the BRT2 check through `targetField`), `forEach` (propagation to a list of
//! neighbours - BRT3), and `compare` (two paths and elementary arithmetic - a
//! product, an offset - BRT4, BRT5). A `compare` path goes through
//! `metadata::value_at`, so it **crosses references** and knows the versions'
//! spellings: BRT5 is written with nothing new, exactly as the design
//! promised. What the vocabulary cannot say stays code - the counts against
//! the datasets (BRT6-8) are already `Property::check` and `element_count`,
//! and better.
//!
//! Two qualifiers joined on 24/09/2026 with `trg/geometry-local-crs`: a rule
//! may be `nested` (its `source` is looked for inside each object - a geometry
//! is never an object of its own), and a `targets` check may carry `notes` (a
//! sentence per wrong target a decision has named, appended to the finding).
//! A third joined on 29/09/2026 with `trg/contact-element-is-geological`: a
//! `targets` check may carry `through` (`Check::Targets::through`, see
//! [`Through`]) - a rejected target opened and read one hop further, reported
//! at its **own** gravity and wording instead of silence or the rule's own
//! severity, for the one wrong target a decision named rather than every one.
//!
//!
//! # Three outcomes, not two
//!
//! The distinction `QuantityClassNotCheckable` established: a rule that
//! applies and that the file breaks is a **violation**; a rule whose
//! condition does not hold is **inapplicable** - silent; a rule that applies
//! but whose reading failed (a path not found, a target outside the package)
//! is **uncheckable**, never a silent "pass" - a path misspelt in a rule must
//! not validate every package in the world.
//!
//!
//! In the `when` condition, an **absent** field is not nonsense (the house
//! reading policy): the condition does not hold, the rule is inapplicable. A
//! path that does not *read*, on the other hand, is uncheckable.

use crate::metadata;
use crate::source::ObjectSource;
use energyml_core::identity;
use energyml_core::path;
use energyml_core::value::{FieldValue, Reflect};
use serde_json::Value;

/// What a rule checks - the closed vocabulary of section 3.2 of the design.
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    /// The reference at the end of `path` aims at **one** of the `targets`
    /// concepts (`type_key`s, hence valid for every version that carries
    /// them - and several, because a version sometimes respells the
    /// target: `StringTableLookup` in 2.0.1, `ColumnBasedTable` in 2.2).
    /// The path may cross references: `RepresentedInterpretation.
    /// InterpretedFeature` reads the interpretation to find the feature.
    Targets {
        /// The dotted path to the reference, on the source object.
        path: String,
        /// The accepted `type_key`s - one is enough.
        targets: Vec<String>,
        /// Accept a **subtype** of one of the concepts too - what the TRG
        /// requires when it types a target by its abstract class
        /// (`SupportingRepresentation` → `AbstractRepresentation`).
        ///
        /// The cost is the threshold: the reference's key reads with nothing
        /// opened, so exact equality stays free; inheritance, on the other
        /// hand, forces **resolving the target** and is only tried as a
        /// fallback. A target outside the package then becomes *uncheckable*,
        /// since what it inherits from cannot be known, and never a violation.
        subtypes: bool,
        /// Read the path **without** the release synonyms, so the rule speaks
        /// for one spelling only.
        ///
        /// The default is off, and that is the useful default: a path is
        /// written once and run against every release, which is exactly why
        /// `metadata` treats `RepresentedInterpretation` and `RepresentedObject`
        /// as one field. But two releases sometimes *disagree* about what a
        /// field may hold, and then a release-blind rule cannot be right for
        /// both. RESQML 2.2 lets a representation name a technical feature and
        /// 2.0.1 does not, so the two statements are two rules — and each has
        /// to be able to say which element it is about, or the 2.0.1 one flags
        /// every conforming 2.2 file.
        ///
        /// With this on, an object that has no field of that literal name is
        /// **not this rule's business**: silent, like any inapplicable rule,
        /// and never a violation or an uncheckable.
        exact: bool,
        /// A sentence per target concept that is *not* accepted, appended to
        /// the violation's `read` when the reference aims at that concept.
        ///
        /// For the case a decision has named: a rule that knows why a
        /// particular wrong target is there says so beside the finding, rather
        /// than being silenced for it or leaving a reader to rediscover it.
        /// `trg/geometry-local-crs` is the first — a geometry in degrees under
        /// a `Geographic2dCrs` is the exception that was kept,
        /// still reported, and reported *as* that exception. JSON: `"notes":
        /// { "Geographic2dCrs": "…" }`.
        notes: Vec<(String, String)>,
        /// A softer reading of a rejected target, tried **one hop further**
        /// before the rule's own severity is applied — JSON `"through": { … }`,
        /// see [`Through`].
        ///
        /// `trg/contact-element-is-geological` is the first user ("option C"): fesapi points a contact at the model
        /// frontier's `GenericFeatureInterpretation` rather than at the
        /// `CulturalFeature` itself — going through the interpretation the way
        /// every other accepted target in this rule already does. The rejected
        /// target is opened, `through.path` is read **on it**, and a match
        /// against `through.targets` turns the finding into `through`'s own
        /// `gravity`/`says` instead of the rule's — never silence (the letter
        /// of the guide still asks for the feature, not its interpretation),
        /// and never the rule's own severity either (the whole point of
        /// `through` is that it usually disagrees with it).
        through: Option<Through>,
    },
    /// The field at the end of `path` equals `equals` (a lexical comparison:
    /// the wire value, never the Rust variant name, which is the house rule of
    /// the cross-version readers).
    Equals {
        /// The dotted path to the field, on the source object.
        path: String,
        /// The expected lexical value.
        equals: String,
    },
    /// BRT2: the reference at the end of `path` is resolved, then `field` is
    /// read **on the target** and compared. Requires the object source.
    TargetField {
        /// The dotted path to the reference, on the source object.
        path: String,
        /// The dotted path to the field, on the resolved **target**.
        field: String,
        /// The expected lexical value.
        equals: String,
    },
    /// BRT3: propagation - a check applied to **each** element of a list on
    /// the object (a role's neighbours: `Faults`, the references the object
    /// carries itself). `field` is read on each element - and crosses its
    /// reference, so a `Vec<DataObjectReference>` and a wrapper
    /// (`Horizons[*].Horizon.Domain`) are written the same way - and the
    /// expected value comes from a path on the **source** object (`sameAs`:
    /// that is the propagation) or from a constant (`equals`). An empty list
    /// holds: a `forAll` over nothing is true.
    ForEach {
        /// The path to the list, on the source object.
        each: String,
        /// The path to the field, on **each** element (references crossed).
        field: String,
        /// What each element must say.
        expect: Expect,
    },
    /// BRT4, BRT5: two paths compared, with the elementary arithmetic the
    /// 2016 rules use - a product of factors and a constant offset. The
    /// relation checked is
    /// `compare  op  to x PI(times) + plus`.
    Compare {
        /// Read the paths **without** the release synonyms — see
        /// [`Check::Targets::exact`], which it means exactly the same way.
        exact: bool,
        /// The path to the value being judged.
        compare: String,
        /// The relation - equality, or "at most".
        op: Op,
        /// The path to the reference value.
        to: String,
        /// Multiplicative factors - `Nk.Nj.Ni` is written this way. Empty, the
        /// right-hand side is `to` alone.
        times: Vec<String>,
        /// A constant offset - a lattice's spacing counts are `count - 1`, and
        /// that `-1` is what this carries.
        plus: f64,
    },
}

/// A `Check::Targets`'s `through` fallback: read **one hop further** on the
/// rejected target, and if that reaches one of `targets`, report the finding
/// at this severity and with this wording instead of the check's own.
///
/// Deliberately narrower than `Check::Targets` itself: no `notes`, no nested
/// `through` — a fallback that itself needed a fallback would say the rule
/// does not know what it wants to say about the target it is looking at.
#[derive(Debug, Clone, PartialEq)]
pub struct Through {
    /// The dotted path, read **on the rejected target** and appended to the
    /// check's own `path` (`"Subject"` + `"InterpretedFeature"` reads the
    /// interpretation `Subject` names to find what it interprets) — the same
    /// crossing walk `Check::Targets::path` itself relies on.
    pub path: String,
    /// The concepts that make the rejected target acceptable, softened.
    pub targets: Vec<String>,
    /// Accept a subtype of one of `targets`, the same meaning as
    /// `Targets::subtypes`.
    pub subtypes: bool,
    /// The weight of a finding reached this way — independent of the rule's
    /// own `severity`, since the whole point of `through` is that it usually
    /// disagrees with it.
    pub gravity: Gravity,
    /// What the finding says, replacing the rule's `says` for this outcome.
    pub says: String,
}

/// What a `forEach` expects of each element.
#[derive(Debug, Clone, PartialEq)]
pub enum Expect {
    /// The value read **on the source object** at evaluation time - an
    /// organisation's domain holds for its associated interpretations.
    SameAs(String),
    /// A lexical constant.
    Value(String),
}

/// A `compare`'s relation - closed: what BRT4 and BRT5 require.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `=` - equality (exact: the values compared are counts).
    Equal,
    /// `<=` - the judged value does not exceed the reference.
    AtMost,
    /// `<` - strictly below. Not a nicety: several schema rules say *"always
    /// less than"*, and writing them as `<=` would let the degenerate case
    /// through - a trajectory whose start and finish MD are equal has no
    /// length, and is exactly the file someone would be looking for.
    Below,
}

/// How much a broken rule weighs in `check`.
///
/// **A warning by default** — a rule the guides state is worth knowing, and a
/// file that breaks it is often still usable. **An error when declared so**,
/// rule by rule, following the review of the `[ERROR]` model of Lutetia
/// — the property kind, discrete value, trajectory, listric fault, collection
/// and contact anomalies are errors, the series asset anomaly stays a warning. The weight is part of the
/// rule's declaration, never decided by whoever prints the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gravity {
    /// Worth knowing; the file is not condemned.
    #[default]
    Warning,
    /// The file breaks what makes it sound: `check` fails.
    Error,
}

/// A business constraint, as the JSON registry declares it.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// The rule's name - `brt1/fault-interprets-tectonic`; the prefix tells a
    /// rule of the standard from a house rule.
    pub id: String,
    /// The `type_key` of the objects concerned - `FaultInterpretation`, or an
    /// **abstract class** (`AbstractProperty`): the rule then holds for
    /// everything that descends from it. Kinship is read in the object's base
    /// chain, so with no registry and no reading - which is what writes the
    /// TRG's table in eight rules instead of forty.
    pub source: String,
    /// Look for `source` **inside** each object too, at any depth - JSON
    /// `"nested": true`.
    ///
    /// Some concepts the guides state rules about are never data objects of
    /// their own: an `AbstractGeometry` lives in a representation's patch, and
    /// the paths to it differ by representation (`NodePatchGeometry[*]`,
    /// `TrianglePatch[*].Geometry`, `Grid2dPatch.Geometry`, a trajectory's
    /// `Geometry`…). A path per representation would be a rule that forgets
    /// the next one; this walks to every object of the concept, judges each
    /// where it stands, and keeps the top-level object's uuid on the finding,
    /// with the path to the part at the head of `read` or `why`. A match is
    /// not walked into: what it holds is its own business.
    pub nested: bool,
    /// Concepts this rule is **not** about, although they descend from
    /// `source` — JSON `"except": ["WellboreFrameRepresentation", …]`.
    ///
    /// For the case where the guide states a narrower class's own rule: RESQML
    /// TRG 2.2 lets a `WellboreFrameRepresentation` and a
    /// `WellboreTrajectoryRepresentation` represent a WITSML `Wellbore`
    /// (§12.13 p. 310, §12.17 p. 314), which every other representation may
    /// not. The general rule excepts them and a rule of their own states
    /// what they may aim at — rather than one rule too broad for all the
    /// others, or too narrow for these two. Read in the object's base chain,
    /// like `source`, so a subclass of an excepted concept is excepted too.
    pub except: Vec<String>,
    /// The releases this rule speaks for — JSON `"release": "2.0.1"` or a list.
    ///
    /// Compared with the **version of the object's own schema** (`TypeInfo::version`:
    /// `2.0.1`, `2.2`…). Empty, the rule holds for every release. For the case a
    /// guide states a rule for one release and the other has none, or states it
    /// differently — BRT1 (a fault interprets a tectonic boundary) has no
    /// counterpart in 2.2, where `TectonicBoundaryFeature` is gone, and run on
    /// a 2.2 fault it would condemn every conforming file.
    pub release: Vec<String>,
    /// The applicability condition - `equals` or `targets`; absent, the rule
    /// applies to all. This is the `implies` of the 2016 rules: BRT4 and BRT5
    /// only judge a `Grid2dRepresentation` if its interpreted feature is a
    /// seismic lattice.
    pub when: Option<Check>,
    /// The checks, which must **all** hold.
    pub checks: Vec<Check>,
    /// What the rule means, for the report.
    pub says: String,
    /// How much breaking it weighs — JSON `"severity": "error"`, a warning
    /// when absent.
    pub gravity: Gravity,
}

/// What an evaluation yields - the inapplicable is silent, on principle.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The rule applies and the document breaks it.
    Violation {
        /// What was read, so the report can show the disagreement.
        read: String,
    },
    /// The rule applies but could not be evaluated - and why.
    Unverifiable {
        /// What prevented the evaluation, named.
        why: String,
    },
}

/// One finding: the rule, the object, the outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// The `id` of the rule evaluated.
    pub rule: String,
    /// The uuid of the object judged.
    pub uuid: String,
    /// What the rule means, copied for the report.
    pub says: String,
    /// The outcome - violation or uncheckable; the inapplicable is silent.
    pub outcome: Outcome,
    /// How much a violation weighs — the rule's.
    pub gravity: Gravity,
}

/// The associations the TRG states and the XSD leaves untyped,
/// transcribed in one go (ADR-0014).
///
/// These are the targets the UML model imposes and that no schema can
/// enforce: a `DataObjectReference` does not declare what it is allowed to
/// aim at, so nothing stops a property naming an interpretation as its
/// support. Every `id` carries the `trg/` prefix: a rule of the standard is
/// told from a house rule in the report, as the colour cascade did.
///
/// Three things make them short. The `source`s are the model's **abstract
/// classes** (one rule holds for every property); the `targets` accept
/// several concepts, because a version respells the target
/// (`StringTableLookup` in 2.0.1, `ColumnBasedTable` in 2.2); and `subtypes`
/// states the cases where the TRG types by the abstract class.
///
/// Two lines of the TRG's table are **not** here, each for a stated reason
/// rather than through an oversight:
///
/// - `AbstractGraphicalInformation.TargetObject` - the model deliberately
///   allows decorating any object at all (section 6), and a rule that
///   accepts everything says nothing;
/// - `AbstractProperty.PropertyKind` and `PropertyKind.Parent` - in RESQML
///   2.0.1 both are `AbstractPropertyKind`, that is, *either* a kind from
///   the dictionary (an enumeration value, not a reference) *or* a DOR to a
///   local kind; 2.2 made it an outright reference. The rule vocabulary
///   types references and cannot say "one or the other", and a rule holding
///   for only half the packages would be worse than no rule. What those two
///   lines wanted is already held, and better, by `property_kind`: the
///   package outranks the dictionary, and a kind not found is an error that
///   names both places searched.
///
/// # The two `LocalCrs` rules, and why the geometry's is nested
///
/// RESQML TRG 2.2 types both associations the XSD leaves open: *"From:
/// AbstractGeometry.LocalCrs To: LocalEngineeringCompoundCrs"* (§3.1 p. 22)
/// and *"From: AbstractProperty.LocalCrs 0..1 To: LocalEngineeringCompoundCrs"*
/// (§7.1 p. 222). RESQML 2.0.1's `AbstractLocal3dCrs` is `LocalDepth3dCrs` or
/// `LocalTime3dCrs`, which the release notes of 2.2 (p. 3) say the compound
/// replaced — so both rules accept the three.
///
/// `trg/geometry-local-crs` is `nested` because a geometry is never an object
/// of its own (see [`Rule::nested`]). It was missing at first:
/// every geometry `energyml-build` wrote
/// named a `ProjectedCrs` or a `Geographic2dCrs`, and nothing said so. It
/// declares no `severity`, so a violation is a **warning** in `check` — the
/// packages already written are readable, and 20 objects of the BRGM package
/// break it. (Rules are not all warnings: `trg/contact-element-is-geological`
/// is an error, see [`Gravity`].) The exception decided on 24/09 — a geometry in degrees
/// keeps its `Geographic2dCrs`, EML 2.3 having no compound over a geographic
/// system — is still reported, **named in the finding** by its note.
///
/// # The contact rule, and what "geological" and "frontier" are taken to mean
///
/// RESQML TRG 2.2 §6.9 p. 173, on `ContactElement`: *"BUSINESS RULE: The
/// content type of the corresponding data-object reference must be a
/// geological feature-interpretation or a frontier feature."* The anomaly of the
/// synthetic reference model audit (a contact whose `Subject` is a representation)
/// is what it catches. `nested`, because a contact part is never an object of
/// its own: it lives in an organisation's `ContactInterpretation` list.
///
/// The two words are read through the class tree, and the reading is ours:
///
/// - a *geological* feature interpretation is a `BoundaryFeatureInterpretation`
///   (horizon, fault, geobody boundary, fluid boundary) or a
///   `GeologicUnitInterpretation` (stratigraphic unit, geobody, rock fluid
///   unit…), in both releases. Left out on purpose: the organisations, the
///   earth model, the wellbore and the `GenericFeatureInterpretation` — none of
///   them is a thing a contact can lie between;
/// - a *frontier feature* is 2.0.1's `FrontierFeature`, and in 2.2 a
///   `CulturalFeature` — the class RESQML 2.2 keeps a model frontier in
///   (Interpretations.xsd l. 994–1008: *"either points to a CulturalFeature
///   whose Kind is model frontier or to a BoundaryFeatureInterpretation"*).
///   Its `Kind` is not read: a typing rule reads what the reference names,
///   and a cultural feature of another kind is a narrower defect than the one
///   this rule exists for.
///
/// # A frontier reached through its interpretation — `through`, option C
///
/// The finding, on `testingPackage`: a contact relates geological feature
/// interpretations or a frontier feature, never a representation (RESQML TRG 2.2
/// §6.9 p. 173: the content type of a ContactElement must be a geological
/// feature-interpretation or a frontier feature); read:
/// ContactInterpretation[4]: GenericFeatureInterpretation, in StructuralOrg1
/// Interp1 resqml22.StructuralOrganizationInterpretation.
///
/// `testdata/testingPackageCpp22.epc` (written by fesapi) has a contact whose
/// `Subject` is `X Minus Frontier Interp`, a `GenericFeatureInterpretation`
/// whose own `InterpretedFeature` is the `CulturalFeature` that is the
/// model's frontier — fesapi reaches the frontier **through its
/// interpretation**, the way every other accepted target in this rule already
/// is an interpretation. The letter of TRG 2.2 §6.9 p. 173 asks for the
/// frontier *feature* itself, not an interpretation of it, so this is not
/// nothing; but the intent is identifiable one hop away, and fesapi is a
/// reference implementation, not a fringe producer. The decision
/// is option C of the three considered (silence, keep the
/// error, or soften it) — so a `GenericFeatureInterpretation`
/// (or any interpretation) reached as `Subject`/`DirectObject`, whose
/// `InterpretedFeature` is itself a `CulturalFeature` or `FrontierFeature`, is
/// no longer an error: it is a **named warning** (`through`, see [`Through`]),
/// and every other wrong target — a representation, an interpretation of
/// anything else (a seismic line, say) — stays an error exactly as before.
const TRG: &str = r#"[
  {
    "id": "trg/property-supporting-representation",
    "source": "AbstractProperty",
    "check": { "path": "SupportingRepresentation", "targets": "AbstractRepresentation", "subtypes": true },
    "says": "a property is supported by a representation"
  },
  {
    "id": "trg/property-local-crs",
    "source": "AbstractProperty",
    "check": { "path": "LocalCrs", "targets": ["LocalEngineeringCompoundCrs", "LocalDepth3dCrs", "LocalTime3dCrs"] },
    "says": "a property's local CRS is a complete local system"
  },
  {
    "id": "trg/geometry-local-crs",
    "source": "AbstractGeometry",
    "nested": true,
    "check": {
      "path": "LocalCrs",
      "targets": ["LocalEngineeringCompoundCrs", "LocalDepth3dCrs", "LocalTime3dCrs"],
      "notes": {
        "Geographic2dCrs": "the named exception: EML 2.3 has no local engineering compound over a geographic system (`LocalEngineering2dCrs.OriginProjectedCrs` is a `ProjectedCrs`, CRS.xsd), so a geometry in degrees keeps its `Geographic2dCrs` (decided 24/09/2026); it states no vertical datum and no vertical axis",
        "ProjectedCrs": "the horizontal half alone: read as written, but no vertical datum and no direction of z are stated anywhere this geometry points; a `LocalEngineeringCompoundCrs` over it is what the guide asks for"
      }
    },
    "says": "a geometry's local CRS is a complete local system (RESQML TRG 2.2 §3.1 p. 22: AbstractGeometry.LocalCrs to LocalEngineeringCompoundCrs)"
  },
  {
    "id": "trg/discrete-category-lookup",
    "source": "CategoricalProperty",
    "check": { "path": "CategoryLookup", "targets": ["StringTableLookup", "ColumnBasedTable"] },
    "says": "a categorical property's category table is a lookup table (RESQML 2.0.1: CategoricalProperty.Lookup to StringTableLookup)"
  },
  {
    "id": "trg/discrete-property-category-lookup",
    "source": "DiscreteProperty",
    "check": { "path": "CategoryLookup", "targets": "ColumnBasedTable", "exact": true },
    "says": "a RESQML 2.2 discrete property's category table is a column-based table (RESQML TRG 2.2 §7.7 p. 230: DiscreteProperty.CategoryLookup 0..1 to ColumnBasedTable)"
  },
  {
    "id": "trg/subrepresentation-support",
    "source": "SubRepresentation",
    "check": { "path": "SupportingRepresentation", "targets": "AbstractRepresentation", "subtypes": true, "exact": true },
    "says": "a RESQML 2.0.1 sub-representation is supported by a representation"
  },
  {
    "id": "trg/subrepresentation-patch-support",
    "source": "SubRepresentationPatch",
    "nested": true,
    "check": { "path": "SupportingRepresentation", "targets": "AbstractRepresentation", "subtypes": true, "exact": true },
    "says": "each patch of a RESQML 2.2 sub-representation is supported by a representation (RESQML TRG 2.2 §8.10 p. 247: SubRepresentationPatch.SupportingRepresentation 1 to AbstractRepresentation)"
  },
  {
    "id": "trg/representation-interprets",
    "source": "AbstractRepresentation",
    "check": { "path": "RepresentedInterpretation", "targets": "AbstractFeatureInterpretation", "subtypes": true, "exact": true },
    "says": "a representation of RESQML 2.0.1 represents an interpretation"
  },
  {
    "id": "trg/representation-represents",
    "source": "AbstractRepresentation",
    "except": ["WellboreFrameRepresentation", "WellboreTrajectoryRepresentation"],
    "check": { "path": "RepresentedObject", "targets": ["AbstractFeatureInterpretation", "AbstractTechnicalFeature"], "subtypes": true, "exact": true },
    "says": "a representation of RESQML 2.2 represents an interpretation or a technical feature"
  },
  {
    "id": "trg/wellbore-frame-represents",
    "source": "WellboreFrameRepresentation",
    "check": { "path": "RepresentedObject", "targets": ["AbstractFeatureInterpretation", "AbstractTechnicalFeature", "Wellbore"], "subtypes": true, "exact": true },
    "says": "a RESQML 2.2 wellbore frame represents an interpretation, a technical feature or a WITSML v2 wellbore (RESQML TRG 2.2 §12.13 p. 310: a WITSML v2 Wellbore can be used as the represented data object for this representation)"
  },
  {
    "id": "trg/wellbore-trajectory-represents",
    "source": "WellboreTrajectoryRepresentation",
    "check": { "path": "RepresentedObject", "targets": ["AbstractFeatureInterpretation", "AbstractTechnicalFeature", "Wellbore"], "subtypes": true, "exact": true },
    "says": "a RESQML 2.2 wellbore trajectory represents an interpretation, a technical feature or a WITSML v2 wellbore (RESQML TRG 2.2 §12.17 p. 314: a WITSML v2 Wellbore can be used as the represented data object for this representation)"
  },
  {
    "id": "trg/trajectory-md-increases",
    "source": "WellboreTrajectoryRepresentation",
    "check": { "compare": "StartMd", "op": "<", "to": "FinishMd", "exact": true },
    "says": "a RESQML 2.0.1 trajectory starts at a smaller MD than it finishes at"
  },
  {
    "id": "trg/trajectory-md-interval-increases",
    "source": "WellboreTrajectoryRepresentation",
    "check": { "compare": "MdInterval.MdMin", "op": "<", "to": "MdInterval.MdMax", "exact": true },
    "says": "a RESQML 2.2 trajectory's MD interval starts below where it ends"
  },
  {
    "id": "trg/boundary-interpretation-interprets-boundary",
    "source": "BoundaryFeatureInterpretation",
    "check": { "path": "InterpretedFeature", "targets": "BoundaryFeature", "subtypes": true },
    "says": "a boundary interpretation interprets a boundary feature"
  },
  {
    "id": "trg/geologic-unit-interprets-rock-volume",
    "source": "GeologicUnitInterpretation",
    "check": { "path": "InterpretedFeature", "targets": ["RockVolumeFeature", "GeologicUnitFeature"], "subtypes": true },
    "says": "a geologic unit interpretation interprets a rock volume feature"
  },
  {
    "id": "trg/earth-model-interprets-model",
    "source": "EarthModelInterpretation",
    "check": { "path": "InterpretedFeature", "targets": ["Model", "OrganizationFeature"] },
    "says": "an earth model interpretation interprets only a model feature (RESQML 2.2 Interpretations.xsd l. 632, TRG 2.2 §6.19 p. 187; in 2.0.1 an organisation feature)"
  },
  {
    "id": "trg/interpretation-interprets-feature",
    "source": "AbstractFeatureInterpretation",
    "check": { "path": "InterpretedFeature", "targets": "AbstractFeature", "subtypes": true },
    "says": "an interpretation interprets a feature"
  },
  {
    "id": "trg/contact-element-is-geological",
    "source": "BinaryContactInterpretationPart",
    "nested": true,
    "severity": "error",
    "checks": [
      {
        "path": "Subject",
        "targets": ["BoundaryFeatureInterpretation", "GeologicUnitInterpretation", "FrontierFeature", "CulturalFeature"],
        "subtypes": true,
        "through": {
          "path": "InterpretedFeature",
          "targets": ["CulturalFeature", "FrontierFeature"],
          "severity": "warning",
          "says": "a frontier reached through its interpretation — the TRG asks for the frontier feature itself (RESQML TRG 2.2 §6.9 p. 173), not the interpretation fesapi points at"
        }
      },
      {
        "path": "DirectObject",
        "targets": ["BoundaryFeatureInterpretation", "GeologicUnitInterpretation", "FrontierFeature", "CulturalFeature"],
        "subtypes": true,
        "through": {
          "path": "InterpretedFeature",
          "targets": ["CulturalFeature", "FrontierFeature"],
          "severity": "warning",
          "says": "a frontier reached through its interpretation — the TRG asks for the frontier feature itself (RESQML TRG 2.2 §6.9 p. 173), not the interpretation fesapi points at"
        }
      }
    ],
    "says": "a contact relates geological feature interpretations or a frontier feature, never a representation (RESQML TRG 2.2 §6.9 p. 173: the content type of a ContactElement must be a geological feature-interpretation or a frontier feature)"
  },
  {
    "id": "trg/brt1-fault-interprets-tectonic",
    "source": "FaultInterpretation",
    "release": "2.0.1",
    "check": { "path": "InterpretedFeature", "targets": "TectonicBoundaryFeature" },
    "says": "a RESQML 2.0.1 fault interpretation interprets a tectonic boundary feature (BRT1 of the Energistics 2016 business-rule delivery; the RESQML validator guide 2017 §5.3.1 p. 36 calls it implicit, not written in the 2.0.1 text — a house reading, hence a warning. 2.2 has no TectonicBoundaryFeature: trg/boundary-interpretation-interprets-boundary states it there)"
  },
  {
    "id": "trg/brt2-earth-model-organization-kind",
    "source": "EarthModelInterpretation",
    "release": "2.0.1",
    "when": { "path": "InterpretedFeature", "targets": "OrganizationFeature" },
    "check": { "path": "InterpretedFeature", "field": "OrganizationKind", "equals": "earth model" },
    "says": "a RESQML 2.0.1 earth model interpretation interprets an organization feature of kind earth model (RESQML 2.0.1 Interpretations.xsd l. 480, obj_EarthModelInterpretation: BUSINESS RULE: An earth model Interpretation interprets only an earth model feature; BRT2 of the 2016 delivery; validator guide 2017 §5.3.2 p. 37)"
  },
  {
    "id": "trg/brt4-seismic-grid2d-lattice-counts",
    "source": "Grid2dRepresentation",
    "release": "2.0.1",
    "when": { "path": "RepresentedInterpretation.InterpretedFeature", "targets": "SeismicLatticeFeature" },
    "checks": [
      { "compare": "Grid2dPatch.Geometry.Points.Offset[0].Spacing.Count", "op": "=", "to": "Grid2dPatch.SlowestAxisCount", "plus": -1 },
      { "compare": "Grid2dPatch.Geometry.Points.Offset[1].Spacing.Count", "op": "=", "to": "Grid2dPatch.FastestAxisCount", "plus": -1 }
    ],
    "says": "the spacing of a lattice of N points has N-1 values, slowest axis first (RESQML 2.0.1 Geometry.xsd l. 22, Point3dLatticeArray.Offset.Spacing: A lattice of N offset points is described by a spacing array of size N-1; axis order RESQML TRG 2.2 §3.17 p. 40; BRT4 of the 2016 delivery, which judges seismic grids)"
  },
  {
    "id": "trg/brt4-seismic-grid2d-lattice-counts-22",
    "source": "Grid2dRepresentation",
    "release": "2.2",
    "when": { "path": "RepresentedObject", "targets": "SeismicLatticeFeature", "exact": true },
    "checks": [
      { "compare": "Geometry.Points.Dimension[0].Spacing.Count", "op": "=", "to": "SlowestAxisCount", "plus": -1, "exact": true },
      { "compare": "Geometry.Points.Dimension[1].Spacing.Count", "op": "=", "to": "FastestAxisCount", "plus": -1, "exact": true }
    ],
    "says": "the spacing of a lattice of N points has N-1 values, slowest axis first (RESQML 2.2 Geometry.xsd l. 415, Point3dLatticeDimension.Spacing: A lattice of N offset points is described by a spacing array of size N-1; RESQML TRG 2.2 §3.17 p. 40: the first given axis is the slowest dimension)"
  },
  {
    "id": "trg/brt5-grid2d-within-seismic-lattice",
    "source": "Grid2dRepresentation",
    "release": "2.0.1",
    "when": { "path": "RepresentedInterpretation.InterpretedFeature", "targets": "SeismicLatticeFeature" },
    "checks": [
      { "compare": "Grid2dPatch.FastestAxisCount", "op": "<=", "to": "RepresentedInterpretation.InterpretedFeature.InlineCount" },
      { "compare": "Grid2dPatch.SlowestAxisCount", "op": "<=", "to": "RepresentedInterpretation.InterpretedFeature.CrosslineCount" }
    ],
    "says": "a grid 2D on a seismic lattice does not exceed its inlines (fastest axis) and crosslines (slowest axis) (BRT5 of the 2016 delivery; RESQML 2.0.1 Technical.xsd l. 103 and l. 123, CrosslineCount and InlineCount: the counts of the survey; no sentence of the 2.0.1 text states it, so a house reading of the validator's rule)"
  },
  {
    "id": "trg/brt5-seismic-lattice-labels-match-grid2d-22",
    "source": "Grid2dRepresentation",
    "release": "2.2",
    "when": { "path": "RepresentedObject", "targets": "SeismicLatticeFeature", "exact": true },
    "checks": [
      { "compare": "FastestAxisCount", "op": "=", "to": "RepresentedObject.InlineLabels.Offset[0].Count", "plus": 1, "exact": true },
      { "compare": "SlowestAxisCount", "op": "=", "to": "RepresentedObject.CrosslineLabels.Offset[0].Count", "plus": 1, "exact": true }
    ],
    "says": "the labels of a seismic lattice are as many as the nodes of the grid 2D's axes: inlines on the fastest, crosslines on the slowest (RESQML 2.2 Seismic.xsd l. 251 InlineLabels and l. 244 CrosslineLabels, BUSINESS RULE: Count of this array must be the same as the count of nodes in the fastest/slowest axis of the associated grid 2D representations; a label array of Count n-1 holds n values, common TRG 2.3 §6.42 p. 128)"
  }
]"#;

/// The TRG rule set, ready to use - the equivalent of `Spec::standard()` for
/// metadata: shipped configuration, not code. It is **re-read on every call**
/// rather than held in a static, because a rule set is read once per
/// validation and a global cache would be one more thing to invalidate.
///
/// The `expect` is held by a test: a shipped set that does not parse would be
/// a library that cannot read its own format.
pub fn trg() -> Vec<Rule> {
    parse(TRG).expect("the shipped TRG rule set parses")
}

/// Read a rule registry from its JSON - refused naming the rule and the
/// field, never a silently partial registry.
pub fn parse(json: &str) -> Result<Vec<Rule>, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| format!("the rule set is not JSON: {e}"))?;
    let list = root.as_array().ok_or("a rule set is a JSON array of rules")?;
    let mut out = Vec::with_capacity(list.len());
    for (at, entry) in list.iter().enumerate() {
        let text = |name: &str| -> Result<String, String> {
            entry[name]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("rule {at}: `{name}` is required"))
        };
        let id = text("id")?;
        // **An unknown key is a refusal**, never a silently weaker rule: a typo
        // (`"severty": "error"`, `"whne"`, `"subtype"`) used to give a warning, a
        // rule with no condition or one with no inheritance, without a word —
        // the *silently partial registry* this parser exists to refuse.
        if let Some(map) = entry.as_object() {
            const KEYS: &[&str] = &[
                "id", "source", "nested", "except", "release", "when", "check", "checks", "says", "severity",
            ];
            if let Some(unknown) = map.keys().find(|key| !KEYS.contains(&key.as_str())) {
                return Err(format!(
                    "rule `{id}`: unknown key `{unknown}` (known: {})",
                    KEYS.join(", ")
                ));
            }
        }
        let mut checks = Vec::new();
        let raw_checks = match entry["checks"].as_array() {
            Some(list) => list.clone(),
            None if !entry["check"].is_null() => vec![entry["check"].clone()],
            None => return Err(format!("rule `{id}`: `check` or `checks` is required")),
        };
        for check in &raw_checks {
            checks.push(parse_check(&id, check)?);
        }
        let when = match &entry["when"] {
            Value::Null => None,
            condition => match parse_check(&id, condition)? {
                found @ (Check::Equals { .. } | Check::Targets { .. }) => Some(found),
                _ => {
                    return Err(format!(
                        "rule `{id}`: `when` is `equals` or `targets` — a condition reads, it does not compute"
                    ));
                }
            },
        };
        let nested = match &entry["nested"] {
            Value::Null => false,
            Value::Bool(flag) => *flag,
            _ => return Err(format!("rule `{id}`: `nested` is a boolean")),
        };
        let except: Vec<String> = match &entry["except"] {
            Value::Null => Vec::new(),
            Value::String(one) if !one.is_empty() => vec![one.clone()],
            Value::Array(list) if !list.is_empty() => list
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .filter(|v| !v.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| format!("rule `{id}`: every `except` entry is a concept name"))
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => {
                return Err(format!(
                    "rule `{id}`: `except` is a concept name or a list of them"
                ));
            }
        };
        let release: Vec<String> = match &entry["release"] {
            Value::Null => Vec::new(),
            Value::String(one) if !one.is_empty() => vec![one.clone()],
            Value::Array(list) if !list.is_empty() => list
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .filter(|v| !v.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| {
                            format!("rule `{id}`: every `release` entry is a version such as `2.0.1`")
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(format!("rule `{id}`: `release` is a version or a list of them")),
        };
        let gravity = match &entry["severity"] {
            Value::Null => Gravity::Warning,
            Value::String(word) if word == "warning" => Gravity::Warning,
            Value::String(word) if word == "error" => Gravity::Error,
            other => {
                return Err(format!(
                    "rule `{id}`: `severity` is \"warning\" or \"error\", not {other}"
                ));
            }
        };
        out.push(Rule {
            id,
            source: text("source")?,
            nested,
            except,
            release,
            when,
            checks,
            says: text("says")?,
            gravity,
        });
    }
    Ok(out)
}

/// One check, from its JSON form - `compare` first (it has no `path`), then
/// the vocabulary keyed by path.
fn parse_check(id: &str, check: &Value) -> Result<Check, String> {
    const KEYS: &[&str] = &[
        "path", "targets", "subtypes", "exact", "notes", "through", "equals", "field", "compare", "op", "to",
        "times", "plus", "each", "sameAs",
    ];
    if let Some(map) = check.as_object()
        && let Some(unknown) = map.keys().find(|key| !KEYS.contains(&key.as_str()))
    {
        return Err(format!(
            "rule `{id}`: unknown check key `{unknown}` (known: {})",
            KEYS.join(", ")
        ));
    }
    if let Some(each) = check["each"].as_str().filter(|v| !v.is_empty()) {
        let field = check["field"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("rule `{id}`: `each` needs a `field` to read on every element"))?
            .to_owned();
        let expect = match (check["sameAs"].as_str(), check["equals"].as_str()) {
            (Some(path), None) if !path.is_empty() => Expect::SameAs(path.to_owned()),
            (None, Some(value)) => Expect::Value(value.to_owned()),
            _ => {
                return Err(format!(
                    "rule `{id}`: `each` expects `sameAs` (a source path) or `equals` (a value) — exactly one"
                ));
            }
        };
        return Ok(Check::ForEach {
            each: each.to_owned(),
            field,
            expect,
        });
    }
    if let Some(compare) = check["compare"].as_str().filter(|v| !v.is_empty()) {
        let op = match check["op"].as_str() {
            Some("=") | None => Op::Equal,
            Some("<=") => Op::AtMost,
            Some("<") => Op::Below,
            Some(other) => {
                return Err(format!("rule `{id}`: `op` is `=`, `<=` or `<`, not `{other}`"));
            }
        };
        let to = check["to"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("rule `{id}`: `compare` needs a `to` path"))?
            .to_owned();
        let times = match &check["times"] {
            Value::Null => Vec::new(),
            Value::Array(list) => list
                .iter()
                .map(|factor| {
                    factor
                        .as_str()
                        .filter(|v| !v.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| format!("rule `{id}`: every `times` entry is a path"))
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(format!("rule `{id}`: `times` is an array of paths")),
        };
        let plus = match &check["plus"] {
            Value::Null => 0.0,
            value => value
                .as_f64()
                .ok_or_else(|| format!("rule `{id}`: `plus` is a number"))?,
        };
        let exact = match &check["exact"] {
            Value::Null => false,
            Value::Bool(flag) => *flag,
            _ => return Err(format!("rule `{id}`: `exact` is a boolean")),
        };
        return Ok(Check::Compare {
            exact,
            compare: compare.to_owned(),
            op,
            to,
            times,
            plus,
        });
    }
    let path = check["path"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("rule `{id}`: every check needs a `path` (or a `compare`)"))?
        .to_owned();
    // `targets` is a string **or** a list: a version sometimes respells the
    // target, and a rule that could only name one would have to be written
    // twice to say one thing.
    let targets: Option<Vec<String>> = match &check["targets"] {
        Value::Null => None,
        Value::String(one) if !one.is_empty() => Some(vec![one.clone()]),
        Value::Array(list) if !list.is_empty() => Some(
            list.iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .filter(|v| !v.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| format!("rule `{id}`: every `targets` entry is a concept name"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        _ => {
            return Err(format!(
                "rule `{id}`: `targets` is a concept name or a list of them"
            ));
        }
    };
    let subtypes = match &check["subtypes"] {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        _ => return Err(format!("rule `{id}`: `subtypes` is a boolean")),
    };
    let exact = match &check["exact"] {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        _ => return Err(format!("rule `{id}`: `exact` is a boolean")),
    };
    let notes: Vec<(String, String)> = match &check["notes"] {
        Value::Null => Vec::new(),
        Value::Object(map) if targets.is_some() => map
            .iter()
            .map(|(concept, note)| {
                note.as_str()
                    .filter(|v| !v.is_empty())
                    .map(|note| (concept.clone(), note.to_owned()))
                    .ok_or_else(|| {
                        format!("rule `{id}`: every `notes` entry is a concept name and a sentence")
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Value::Object(_) => {
            return Err(format!(
                "rule `{id}`: `notes` qualifies `targets`, and there are none"
            ));
        }
        _ => return Err(format!("rule `{id}`: `notes` maps a concept name to a sentence")),
    };
    let through: Option<Through> = match &check["through"] {
        Value::Null => None,
        Value::Object(_) if targets.is_some() => Some(parse_through(id, &check["through"])?),
        Value::Object(_) => {
            return Err(format!(
                "rule `{id}`: `through` qualifies `targets`, and there are none"
            ));
        }
        _ => return Err(format!("rule `{id}`: `through` is an object")),
    };
    match (targets, check["equals"].as_str(), check["field"].as_str()) {
        (Some(targets), None, None) => Ok(Check::Targets {
            path,
            targets,
            subtypes,
            exact,
            notes,
            through,
        }),
        (None, Some(equals), None) => Ok(Check::Equals {
            path,
            equals: equals.to_owned(),
        }),
        (None, Some(equals), Some(field)) => Ok(Check::TargetField {
            path,
            field: field.to_owned(),
            equals: equals.to_owned(),
        }),
        _ => Err(format!(
            "rule `{id}`: a check is `targets`, `equals`, `field`+`equals` or `compare` — nothing else"
        )),
    }
}

/// A `Targets` check's `through`, from its JSON form.
fn parse_through(id: &str, value: &Value) -> Result<Through, String> {
    let path = value["path"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("rule `{id}`: `through.path` is required"))?
        .to_owned();
    let targets: Vec<String> = match &value["targets"] {
        Value::String(one) if !one.is_empty() => vec![one.clone()],
        Value::Array(list) if !list.is_empty() => list
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| format!("rule `{id}`: every `through.targets` entry is a concept name"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(format!(
                "rule `{id}`: `through.targets` is a concept name or a list of them"
            ));
        }
    };
    let subtypes = match &value["subtypes"] {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        _ => return Err(format!("rule `{id}`: `through.subtypes` is a boolean")),
    };
    let gravity = match &value["severity"] {
        Value::Null => Gravity::Warning,
        Value::String(word) if word == "warning" => Gravity::Warning,
        Value::String(word) if word == "error" => Gravity::Error,
        other => {
            return Err(format!(
                "rule `{id}`: `through.severity` is \"warning\" or \"error\", not {other}"
            ));
        }
    };
    let says = value["says"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("rule `{id}`: `through.says` is required"))?
        .to_owned();
    Ok(Through {
        path,
        targets,
        subtypes,
        gravity,
        says,
    })
}

/// The `type_key` of a reference's target, read from the reference itself -
/// ContentType or QualifiedType, with no object resolved.
fn target_key(reference: &dyn Reflect) -> Option<String> {
    identity::key_of_reflect(reference).map(|key| key.type_key().to_owned())
}

/// An object's concepts: its own, then the ones it descends from.
///
/// Read in the base chain the generator laid down - no registry, no reading,
/// no allocation beyond the vector. That is what makes
/// `source: "AbstractProperty"` free on a `ContinuousProperty`.
pub(crate) fn concepts_of(object: &dyn Reflect) -> Vec<&'static str> {
    let info = object.type_info();
    let mut out = vec![info.type_key];
    let mut base = info.base;
    while let Some(next) = base {
        let info = next();
        out.push(info.type_key);
        base = info.base;
    }
    out
}

/// Is this object of the named concept, itself or by inheritance?
fn is_concept(object: &dyn Reflect, concept: &str) -> bool {
    concepts_of(object).contains(&concept)
}

/// The lexical value at the end of a path - `None` when the path does not
/// resolve (the violation/uncheckable distinction belongs to the caller).
fn string_at(object: &dyn Reflect, path: &str) -> Result<Option<String>, String> {
    path::get_string(object, path).map_err(|e| format!("{e}"))
}

/// Evaluate a rule set over objects, with an [`ObjectSource`] for everything
/// that has to read beyond the object being judged - a `targetField`'s
/// target, a `compare` path that crosses a reference.
///
/// `objects` supplies each object **with its uuid**; `source` yields the
/// target of a uuid when the package holds it - `None` is an answer (the
/// target is outside the package, and the finding is *uncheckable*, not a
/// violation).
///
/// `known` is this build's type registry, when the caller has one. It only
/// serves inheritance (`subtypes`), and it beats reading for two reasons: a
/// **type**'s ancestry is static knowledge, so reading it from an *instance*
/// was looking in the wrong place; and a reference that leaves the package -
/// the ordinary case - keeps an answer that way, where opening the target
/// gave none. With no registry the engine falls back on resolution and says
/// when that is not enough.
pub fn run<'a>(
    rules: &[Rule],
    objects: impl Iterator<Item = (&'a str, &'a dyn Reflect)>,
    source: &dyn ObjectSource,
    known: Option<&energyml_core::registry::Registry>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (uuid, object) in objects {
        // The object's concept **and the ones it descends from**: a rule
        // written on `AbstractProperty` holds for every concrete property,
        // which the TRG's table assumes throughout.
        let concepts = concepts_of(object);
        let version = object.type_info().version;
        for rule in rules {
            if !rule.release.is_empty() && !rule.release.iter().any(|one| one == version) {
                continue;
            }
            if rule.nested {
                // The concept lives inside the object: judge every part of it
                // that is one, where it stands.
                let mut parts = Vec::new();
                parts_of(object, &rule.source, String::new(), &mut parts);
                for (at, part) in parts {
                    if rule.except.iter().any(|concept| is_concept(part, concept)) {
                        continue;
                    }
                    judge(rule, uuid, part, &at, source, known, &mut findings);
                }
                continue;
            }
            if !concepts.iter().any(|key| *key == rule.source) {
                continue;
            }
            if rule
                .except
                .iter()
                .any(|concept| concepts.iter().any(|key| key == concept))
            {
                continue;
            }
            judge(rule, uuid, object, "", source, known, &mut findings);
        }
    }
    findings
}

/// One rule against one object — or one part of it, `at` saying where (empty
/// for the object itself). What is read or why it could not be is prefixed
/// with that path, so a finding on a representation's third patch says so.
fn judge(
    rule: &Rule,
    uuid: &str,
    object: &dyn Reflect,
    at: &str,
    source: &dyn ObjectSource,
    known: Option<&energyml_core::registry::Registry>,
    findings: &mut Vec<Finding>,
) {
    let located = |text: String| {
        if at.is_empty() {
            text
        } else {
            format!("{at}: {text}")
        }
    };
    if let Some(condition) = &rule.when {
        match applies(condition, object, source) {
            Applicability::Applies => {}
            // The condition does not hold -> inapplicable, silent.
            Applicability::No => return,
            // Unreadable -> uncheckable: a `when` on a wrong path must not
            // disable the rule over every package.
            Applicability::Cant(why) => {
                findings.push(Finding {
                    rule: rule.id.clone(),
                    uuid: uuid.to_owned(),
                    says: rule.says.clone(),
                    outcome: Outcome::Unverifiable {
                        why: located(format!("`when`: {why}")),
                    },
                    gravity: rule.gravity,
                });
                return;
            }
        }
    }
    for check in &rule.checks {
        let Some(verdict) = evaluate(check, object, source, known) else {
            continue;
        };
        let outcome = match verdict.outcome {
            Outcome::Violation { read } => Outcome::Violation { read: located(read) },
            Outcome::Unverifiable { why } => Outcome::Unverifiable { why: located(why) },
        };
        findings.push(Finding {
            rule: rule.id.clone(),
            uuid: uuid.to_owned(),
            says: verdict.says.unwrap_or_else(|| rule.says.clone()),
            outcome,
            gravity: verdict.gravity.unwrap_or(rule.gravity),
        });
    }
}

/// What one check yields — the outcome, and what should override the rule's
/// own `gravity`/`says` for this one finding, when the check has something
/// more specific to say than the rule as a whole.
///
/// `None` on both keeps the rule's own weight and wording — every check
/// written before 29/09/2026, since the two fields are additive: nothing
/// existing had to change to gain them. `Check::Targets`'s `through` is the
/// only source of an override today, and the reason it exists: the fallback
/// deliberately *disagrees* with the rule's own weight for one specific wrong
/// target, not for the ones the rule's severity is actually about.
struct Verdict {
    outcome: Outcome,
    gravity: Option<Gravity>,
    says: Option<String>,
}

impl Verdict {
    /// The ordinary case: the rule's own `gravity` and `says` apply.
    fn plain(outcome: Outcome) -> Verdict {
        Verdict {
            outcome,
            gravity: None,
            says: None,
        }
    }
}

/// Every part of `object` — itself included — that is of `concept`, with the
/// dotted path that reaches it (XML names, `[i]` for a repeated element).
///
/// A match is not walked into, and fields holding scalars are not read at all:
/// asking for one would build a list of every value of an inline array to find
/// no object in it.
fn parts_of<'a>(
    object: &'a dyn Reflect,
    concept: &str,
    at: String,
    out: &mut Vec<(String, &'a dyn Reflect)>,
) {
    if is_concept(object, concept) {
        out.push((at, object));
        return;
    }
    for field in object.type_info().fields {
        if matches!(field.ty, energyml_core::reflect::FieldType::Scalar(_)) {
            continue;
        }
        let here = if at.is_empty() {
            field.xml_name.to_string()
        } else {
            format!("{at}.{}", field.xml_name)
        };
        match object.field(field.name) {
            Some(FieldValue::Object(child)) => parts_of(child, concept, here, out),
            Some(FieldValue::List(items)) => {
                for (index, item) in items.into_iter().enumerate() {
                    if let FieldValue::Object(child) = item {
                        parts_of(child, concept, format!("{here}[{index}]"), out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The concept a violation read, with the rule's note on it when there is one.
fn noted(notes: &[(String, String)], key: String) -> String {
    match notes.iter().find(|(concept, _)| *concept == key) {
        Some((_, note)) => format!("{key} — {note}"),
        None => key,
    }
}

/// What a `when` condition can answer. Three cases, not two: an **absent**
/// field is not nonsense - the condition does not hold - but a path that
/// does not read is uncheckable.
enum Applicability {
    Applies,
    No,
    Cant(String),
}

fn applies(condition: &Check, object: &dyn Reflect, source: &dyn ObjectSource) -> Applicability {
    match condition {
        Check::Equals { path, equals } => match string_at(object, path) {
            Ok(Some(value)) if &value == equals => Applicability::Applies,
            Ok(Some(_)) => Applicability::No,
            // Absence is not nonsense: the condition does not hold.
            Ok(None) => Applicability::No,
            // The path does not read here - it may cross a reference, which
            // the metadata walk knows how to do.
            Err(why) => match metadata::value_at(object, path, source) {
                Some(value) if &value == equals => Applicability::Applies,
                Some(_) => Applicability::No,
                None => Applicability::Cant(why),
            },
        },
        Check::Targets { path, targets, .. } => match reference_key_at(object, path, source) {
            // A condition never resolves: it says whether the rule is this
            // object's business, and paying for a read to rule it out would
            // make filtering the engine's most expensive item.
            Ok(Some(key)) if targets.iter().any(|wanted| wanted == &key) => Applicability::Applies,
            Ok(Some(_)) => Applicability::No,
            Ok(None) => Applicability::No,
            Err(why) => Applicability::Cant(why),
        },
        // The parser refuses them in `when`; belt and braces.
        _ => Applicability::Cant("a condition is `equals` or `targets`".into()),
    }
}

fn evaluate(
    check: &Check,
    object: &dyn Reflect,
    source: &dyn ObjectSource,
    known: Option<&energyml_core::registry::Registry>,
) -> Option<Verdict> {
    match check {
        Check::Equals { path, equals } => match string_at(object, path) {
            Ok(Some(value)) if &value == equals => None,
            Ok(Some(value)) => Some(Verdict::plain(Outcome::Violation { read: value })),
            Ok(None) => Some(Verdict::plain(Outcome::Unverifiable {
                why: format!("`{path}` holds nothing"),
            })),
            Err(why) => match metadata::value_at(object, path, source) {
                Some(value) if &value == equals => None,
                Some(value) => Some(Verdict::plain(Outcome::Violation { read: value })),
                None => Some(Verdict::plain(Outcome::Unverifiable { why })),
            },
        },
        Check::Targets {
            path,
            targets,
            subtypes,
            exact,
            notes,
            through,
        } => {
            // A rule that speaks for one release's spelling has nothing to say
            // about an object that does not use it. Checked before anything is
            // read: the point is that this object is not the rule's business.
            if *exact && !metadata::declares_exactly(object, path) {
                return None;
            }
            // **An absent association is not a faulty association.**
            // The house reading policy, applied to rules: absence and
            // nonsense are two different pieces of news. A typing rule says
            // *what the reference is allowed to aim at*, so it has nothing
            // to say when there is no reference - and a missing mandatory
            // element is `Validate`'s business, which already says so, in
            // its own place.
            if metadata::absent_at(object, path) {
                return None;
            }
            let (key, matches) = match concept_at(object, path, targets, *subtypes, source, known) {
                Ok(Some(found)) => found,
                Ok(None) => {
                    return Some(Verdict::plain(Outcome::Unverifiable {
                        why: format!("`{path}` is not a readable reference"),
                    }));
                }
                Err(why) => return Some(Verdict::plain(Outcome::Unverifiable { why })),
            };
            if matches {
                return None;
            }
            // The direct target is wrong. `through` may still accept it, one
            // hop further — see [`Through`] for why `trg/contact-element-is-
            // geological` needs exactly this.
            if let Some(through) = through {
                let deeper = format!("{path}.{}", through.path);
                if let Ok(Some((deeper_key, true))) =
                    concept_at(object, &deeper, &through.targets, through.subtypes, source, known)
                {
                    return Some(Verdict {
                        outcome: Outcome::Violation {
                            read: format!("{key} — reached through `{deeper}`: {deeper_key}"),
                        },
                        gravity: Some(through.gravity),
                        says: Some(through.says.clone()),
                    });
                }
            }
            Some(Verdict::plain(Outcome::Violation {
                read: noted(notes, key),
            }))
        }
        Check::TargetField { path, field, equals } => {
            let reference = match reference_at(object, path) {
                Ok(reference) => reference,
                Err(why) => return Some(Verdict::plain(Outcome::Unverifiable { why })),
            };
            let Some(target_uuid) = identity::key_of_reflect(reference).map(|key| key.uuid.to_owned()) else {
                return Some(Verdict::plain(Outcome::Unverifiable {
                    why: format!("`{path}` is not a readable reference"),
                }));
            };
            // Following the reference itself, so the version it names is read.
            let Some(target) = source.object_of(reference) else {
                // Outside the package: nothing to read, and saying so beats
                // validating - the target may be elsewhere and wrong.
                return Some(Verdict::plain(Outcome::Unverifiable {
                    why: format!("the target `{target_uuid}` is not in this package"),
                }));
            };
            match string_at(target.as_reflect(), field) {
                Ok(Some(value)) if &value == equals => None,
                Ok(Some(value)) => Some(Verdict::plain(Outcome::Violation { read: value })),
                Ok(None) => Some(Verdict::plain(Outcome::Unverifiable {
                    why: format!("`{field}` holds nothing on `{target_uuid}`"),
                })),
                Err(why) => Some(Verdict::plain(Outcome::Unverifiable { why })),
            }
        }
        Check::ForEach { each, field, expect } => {
            // The expected value first: without it, nothing is judgeable.
            let wanted = match expect {
                Expect::Value(value) => value.clone(),
                Expect::SameAs(path) => match metadata::value_at(object, path, source) {
                    Some(value) => value,
                    None => {
                        return Some(Verdict::plain(Outcome::Unverifiable {
                            why: format!("`{path}` yields nothing on the source"),
                        }));
                    }
                },
            };
            let items = match path::get(object, each) {
                Ok(FieldValue::List(items)) => items,
                Ok(_) => {
                    return Some(Verdict::plain(Outcome::Unverifiable {
                        why: format!("`{each}` is not a list"),
                    }));
                }
                Err(why) => {
                    return Some(Verdict::plain(Outcome::Unverifiable {
                        why: format!("{why}"),
                    }));
                }
            };
            // One finding for the list, each disagreement **named by its
            // index** - forty findings for forty faults would be a report
            // nobody reads, and "some" with no indices a report nobody can
            // check.
            let mut disagree = Vec::new();
            let mut unreadable = Vec::new();
            for (at, item) in items.iter().enumerate() {
                let FieldValue::Object(element) = item else {
                    unreadable.push(format!("`{each}[{at}]` is not an object"));
                    continue;
                };
                match metadata::value_at(*element, field, source) {
                    Some(value) if value == wanted => {}
                    Some(value) => disagree.push(format!("`{each}[{at}].{field}` = {value}")),
                    None => unreadable.push(format!("`{each}[{at}].{field}` yields nothing")),
                }
            }
            if !disagree.is_empty() {
                // A real violation does not hide behind an unreadable
                // neighbour - it goes first, the unreadable is appended.
                let mut read = format!("{} — expected {wanted}", disagree.join(", "));
                if !unreadable.is_empty() {
                    read.push_str(&format!(" (and unreadable: {})", unreadable.join(", ")));
                }
                Some(Verdict::plain(Outcome::Violation { read }))
            } else if !unreadable.is_empty() {
                Some(Verdict::plain(Outcome::Unverifiable {
                    why: unreadable.join(", "),
                }))
            } else {
                None
            }
        }
        Check::Compare {
            compare,
            op,
            to,
            times,
            plus,
            exact,
        } => {
            // Same escape hatch as `Targets`, for the same reason: a rule
            // written on one release's element has nothing to say about an
            // object that does not declare it, and saying *uncheckable* about
            // every object of the other release is noise, not news.
            if *exact && !metadata::declares_exactly(object, compare) {
                return None;
            }
            // Every read goes through the metadata walk: the paths cross
            // references and know the spellings - BRT5 is BRT4 with a hop,
            // not another mechanism.
            let number = |path: &str| -> Result<f64, String> {
                let text = metadata::value_at(object, path, source)
                    .ok_or_else(|| format!("`{path}` yields nothing"))?;
                text.trim()
                    .parse::<f64>()
                    .map_err(|_| format!("`{path}` holds `{text}`, which is not a number"))
            };
            let left = match number(compare) {
                Ok(value) => value,
                Err(why) => return Some(Verdict::plain(Outcome::Unverifiable { why })),
            };
            let mut right = match number(to) {
                Ok(value) => value,
                Err(why) => return Some(Verdict::plain(Outcome::Unverifiable { why })),
            };
            for factor in times {
                match number(factor) {
                    Ok(value) => right *= value,
                    Err(why) => return Some(Verdict::plain(Outcome::Unverifiable { why })),
                }
            }
            right += plus;
            let holds = match op {
                Op::Equal => left == right,
                Op::AtMost => left <= right,
                Op::Below => left < right,
            };
            if holds {
                None
            } else {
                // Both sides, read: the report shows the disagreement, not a
                // boolean.
                Some(Verdict::plain(Outcome::Violation {
                    read: format!(
                        "`{compare}` = {left}, {} {right}",
                        match op {
                            Op::Equal => "expected",
                            Op::AtMost => "over the allowed",
                            Op::Below => "not below",
                        }
                    ),
                }))
            }
        }
    }
}

/// Whether the reference at `path` on `object` names one of `targets` —
/// directly (the type the reference itself declares, read for free), or, with
/// `subtypes`, by inheritance: the registry first (a type's ancestry is
/// static), the actual target opened through `source` second.
///
/// `Ok(None)` when the reference is not a readable one at all. `Ok(Some((key,
/// false)))` is a *resolved but wrong* target — the ordinary case a `Targets`
/// check reports as a violation — and `Ok(Some((key, true)))` is a match. This
/// is [`Check::Targets`]'s own decision, factored out so [`Through`] can run
/// the exact same test one hop further out, on the rejected target rather
/// than on the object being judged.
fn concept_at(
    object: &dyn Reflect,
    path: &str,
    targets: &[String],
    subtypes: bool,
    source: &dyn ObjectSource,
    known: Option<&energyml_core::registry::Registry>,
) -> Result<Option<(String, bool)>, String> {
    let key = match reference_key_at(object, path, source)? {
        Some(key) => key,
        None => return Ok(None),
    };
    // The concept the reference names, for free.
    if targets.iter().any(|wanted| wanted == &key) {
        return Ok(Some((key, true)));
    }
    if !subtypes {
        return Ok(Some((key, false)));
    }
    // A **type**'s ancestry is static: the registry gives it with nothing
    // opened, and it answers even for a target the package does not hold -
    // the ordinary case of an outgoing reference. A concept this build does
    // not know falls back on resolution below rather than being condemned.
    if let Some(registry) = known {
        let types = registry.types_by_key(&key);
        if !types.is_empty() {
            let descends = types.iter().any(|info| {
                let mut chain = vec![info.type_key];
                let mut base = info.base;
                while let Some(next) = base {
                    let parent = next();
                    chain.push(parent.type_key);
                    base = parent.base;
                }
                targets
                    .iter()
                    .any(|wanted| chain.iter().any(|concept| concept == wanted))
            });
            return Ok(Some((key, descends)));
        }
    }
    // Inheritance: the target has to be opened to know what it descends from.
    // As a fallback only, and missing it is an *error* here — a target that
    // cannot be read is not a faulty one, which the caller reports as
    // uncheckable rather than as a resolved mismatch.
    //
    // The uuid comes from the **same walk** as the key (a path that stops on
    // an object yields its uuid): reading the reference locally would only
    // work for one version's paths - `RepresentedInterpretation` in 2.0.1,
    // `RepresentedObject` in 2.2 - and half the packages would come out
    // uncheckable.
    let Some(target_uuid) = metadata::value_at(object, path, source) else {
        return Err(format!("`{path}` is not a readable reference"));
    };
    // By the uri of what the path reaches, version included.
    let target = match metadata::value_at(object, &format!("{path}.uri"), source) {
        Some(uri) => source.object_at(&uri),
        None => source.object(&target_uuid),
    };
    let Some(target) = target else {
        return Err(format!(
            "`{key}` is not one of {}, and the target `{target_uuid}` is not in this package to say what it descends from",
            targets.join(", ")
        ));
    };
    Ok(Some((
        key,
        targets
            .iter()
            .any(|wanted| is_concept(target.as_reflect(), wanted)),
    )))
}

/// The concept aimed at by the reference at the end of a path - the
/// **local** path first (the precise error), the crossing walk second.
fn reference_key_at(
    object: &dyn Reflect,
    path: &str,
    source: &dyn ObjectSource,
) -> Result<Option<String>, String> {
    match reference_at(object, path) {
        Ok(reference) => Ok(target_key(reference)),
        Err(why) => match metadata::target_key_at(object, path, source) {
            Some(key) => Ok(Some(key)),
            None => Err(why),
        },
    }
}

/// The reference object at the end of a path.
fn reference_at<'a>(object: &'a dyn Reflect, path: &str) -> Result<&'a dyn Reflect, String> {
    match path::get(object, path).map_err(|e| format!("{e}"))? {
        FieldValue::Object(reference) => Ok(reference),
        _ => Err(format!("`{path}` is not a reference")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"[
      {
        "id": "trg/brt1-fault-interprets-tectonic",
        "source": "FaultInterpretation",
        "check": { "path": "InterpretedFeature", "targets": "TectonicBoundaryFeature" },
        "says": "a fault interpretation interprets a tectonic boundary"
      },
      {
        "id": "brt2/earth-model-kind",
        "source": "EarthModelInterpretation",
        "checks": [
          { "path": "InterpretedFeature", "targets": "OrganizationFeature" },
          { "path": "InterpretedFeature", "field": "OrganizationKind", "equals": "earth model" }
        ],
        "says": "an earth model interprets an organisation of kind earth model"
      }
    ]"#;

    #[test]
    fn the_registry_parses_and_refuses_by_name() {
        let rules = parse(RULES).expect("the set parses");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].checks.len(), 1);
        assert_eq!(rules[1].checks.len(), 2);

        let refusal = parse(r#"[{ "id": "x", "source": "Y", "says": "z" }]"#).unwrap_err();
        assert!(refusal.contains("check"), "{refusal}");
        let refusal =
            parse(r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "path": "P" } }]"#).unwrap_err();
        assert!(refusal.contains('x') && refusal.contains("targets"), "{refusal}");
    }

    #[test]
    fn a_compare_check_parses_with_its_arithmetic() {
        let rules = parse(
            r#"[{
              "id": "brt4/axes",
              "source": "Grid2dRepresentation",
              "when": { "path": "RepresentedInterpretation.InterpretedFeature", "targets": "SeismicLatticeFeature" },
              "check": { "compare": "A.Count", "op": "=", "to": "B", "times": ["C"], "plus": -1 },
              "says": "the counts hold"
            }]"#,
        )
        .expect("the compare rule parses");
        assert_eq!(
            rules[0].checks[0],
            Check::Compare {
                exact: false,
                compare: "A.Count".into(),
                op: Op::Equal,
                to: "B".into(),
                times: vec!["C".into()],
                plus: -1.0,
            }
        );
        assert!(matches!(rules[0].when, Some(Check::Targets { .. })));

        // `op` unknown, `to` absent, a `when` that computes: three named refusals.
        let refusal = parse(
            r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "compare": "A", "op": ">", "to": "B" } }]"#,
        )
        .unwrap_err();
        assert!(refusal.contains('>'), "{refusal}");
        let refusal =
            parse(r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "compare": "A" } }]"#).unwrap_err();
        assert!(refusal.contains("to"), "{refusal}");
        let refusal = parse(
            r#"[{ "id": "x", "source": "Y", "says": "z", "when": { "compare": "A", "to": "B" }, "check": { "path": "P", "equals": "v" } }]"#,
        )
        .unwrap_err();
        assert!(refusal.contains("when"), "{refusal}");
    }

    #[test]
    fn nested_and_notes_parse_and_refuse_by_name() {
        let rules = parse(
            r#"[{
              "id": "x",
              "source": "AbstractGeometry",
              "nested": true,
              "check": { "path": "LocalCrs", "targets": "A", "notes": { "B": "why B is wrong" } },
              "says": "z"
            }]"#,
        )
        .expect("a nested rule with notes parses");
        assert!(rules[0].nested);
        assert_eq!(
            rules[0].checks[0],
            Check::Targets {
                path: "LocalCrs".into(),
                targets: vec!["A".into()],
                subtypes: false,
                exact: false,
                notes: vec![("B".into(), "why B is wrong".into())],
                through: None,
            }
        );
        // Absent, `nested` is off: every rule written before it keeps its
        // meaning.
        let plain = parse(RULES).expect("the set parses");
        assert!(plain.iter().all(|rule| !rule.nested));

        // A `nested` that is not a boolean, and notes with no target to
        // qualify: two named refusals.
        let refusal = parse(
            r#"[{ "id": "x", "source": "Y", "says": "z", "nested": "yes", "check": { "path": "P", "targets": "A" } }]"#,
        )
        .unwrap_err();
        assert!(refusal.contains("nested"), "{refusal}");
        let refusal = parse(
            r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "path": "P", "equals": "v", "notes": { "A": "n" } } }]"#,
        )
        .unwrap_err();
        assert!(refusal.contains("notes"), "{refusal}");
    }

    #[test]
    fn a_note_goes_with_its_concept_and_no_other() {
        let notes = [("Geographic2dCrs".to_string(), "the named exception".to_string())];
        assert_eq!(
            noted(&notes, "Geographic2dCrs".into()),
            "Geographic2dCrs — the named exception"
        );
        assert_eq!(noted(&notes, "ProjectedCrs".into()), "ProjectedCrs");
    }

    #[test]
    fn a_through_fallback_parses_with_its_own_weight_and_wording() {
        let rules = parse(
            r#"[{
              "id": "x",
              "source": "Y",
              "check": {
                "path": "Subject",
                "targets": "A",
                "subtypes": true,
                "through": { "path": "InterpretedFeature", "targets": ["B", "C"], "severity": "warning", "says": "softer" }
              },
              "says": "z"
            }]"#,
        )
        .expect("a targets check with `through` parses");
        assert_eq!(
            rules[0].checks[0],
            Check::Targets {
                path: "Subject".into(),
                targets: vec!["A".into()],
                subtypes: true,
                exact: false,
                notes: vec![],
                through: Some(Through {
                    path: "InterpretedFeature".into(),
                    targets: vec!["B".into(), "C".into()],
                    subtypes: false,
                    gravity: Gravity::Warning,
                    says: "softer".into(),
                }),
            }
        );

        // Absent, `through` is off: every check written before it keeps its
        // meaning.
        let plain = parse(RULES).expect("the set parses");
        for rule in &plain {
            for check in &rule.checks {
                if let Check::Targets { through, .. } = check {
                    assert_eq!(*through, None);
                }
            }
        }

        // `through` with no `targets` to qualify, `through.severity` misspelt,
        // and `through` with no `says`: three named refusals.
        let refusal = parse(
            r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "path": "P", "equals": "v", "through": { "path": "Q", "targets": "A", "says": "s" } } }]"#,
        )
        .unwrap_err();
        assert!(refusal.contains("through"), "{refusal}");
        let refusal = parse(
            r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "path": "P", "targets": "A", "through": { "path": "Q", "targets": "B", "severity": "fatal", "says": "s" } } }]"#,
        )
        .unwrap_err();
        assert!(refusal.contains("through.severity"), "{refusal}");
        let refusal = parse(
            r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "path": "P", "targets": "A", "through": { "path": "Q", "targets": "B" } } }]"#,
        )
        .unwrap_err();
        assert!(refusal.contains("through.says"), "{refusal}");
    }

    #[test]
    fn a_for_each_check_parses_with_its_expectation() {
        let rules = parse(
            r#"[{
              "id": "brt3/domain-propagates",
              "source": "StructuralOrganizationInterpretation",
              "checks": [
                { "each": "Faults", "field": "Domain", "sameAs": "Domain" },
                { "each": "Horizons", "field": "Horizon.Domain", "equals": "depth" }
              ],
              "says": "the domain propagates"
            }]"#,
        )
        .expect("the forEach rule parses");
        assert_eq!(
            rules[0].checks[0],
            Check::ForEach {
                each: "Faults".into(),
                field: "Domain".into(),
                expect: Expect::SameAs("Domain".into()),
            }
        );
        assert_eq!(
            rules[0].checks[1],
            Check::ForEach {
                each: "Horizons".into(),
                field: "Horizon.Domain".into(),
                expect: Expect::Value("depth".into()),
            }
        );

        // Neither `sameAs` nor `equals`, or both: a named refusal.
        let refusal =
            parse(r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "each": "L", "field": "F" } }]"#)
                .unwrap_err();
        assert!(refusal.contains("sameAs"), "{refusal}");
        let refusal = parse(
            r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "each": "L", "field": "F", "sameAs": "A", "equals": "b" } }]"#,
        )
        .unwrap_err();
        assert!(refusal.contains("exactly one"), "{refusal}");
    }
}
