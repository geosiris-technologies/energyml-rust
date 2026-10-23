//! The kind of an edge, primary or secondary, and **where each answer comes
//! from**.
//!
//! ETP 1.2 §8.1.1.1.2 (p. 157) lets each ML decide which of its relationships is
//! primary: *"GENERAL RULE: ML-specific rules determine whether a relationship is
//! primary or secondary. The ML rules to apply are determined by one of the data
//! objects in the relationship"* - for a DOR, the ML of the object that holds it;
//! for a by-value relationship, the ML of the container. §8.2.2 (rule 1.a,
//! p. 167-168) gives the one complete ML rule the specification states: *"In
//! RESQML, all relationships are primary"*. The implementation guides that would
//! state the others (note 1 of §8.1.1.1.2) are not published for ETP 1.2.
//!
//! So the answer has four grades, and each is labelled ([`KindBasis`]) so that a
//! derived answer never passes for a stated rule, nor a default for either:
//!
//! | ML of the holder | Kind | Basis |
//! |---|---|---|
//! | RESQML 2.0.1, 2.2 (and its development drops) | always primary | [`KindBasis::EtpSpec`] §8.2.2, p. 167-168 |
//! | WITSML 2.0, 2.1: the three examples of §8.1.1.1.2, p. 157 | `Log` contains `ChannelSet`, `ChannelSet` contains `Channel`: primary; `Channel` to `Wellbore`: secondary | [`KindBasis::EtpSpec`] |
//! | WITSML 2.0, 2.1, EML, PRODML: any other by-value relationship | primary | [`KindBasis::DerivedByValue`] |
//! | WITSML 2.0, 2.1, EML, PRODML: any other DOR the schema tables know | primary when the XSD makes it mandatory, secondary otherwise | [`KindBasis::DerivedXsd`] |
//! | any ML: a DOR the tables hold no fact for; any other ML; a source that is not an object URI | primary | [`KindBasis::Default`] |
//!
//! The derived rows are this crate's reading of the characterisation of
//! §8.1.1.1.2 (p. 157), not a text of the specification, and are never labelled
//! as one. Primary: *"One end of the relationship is mandatory; that is, one
//! object cannot exist (as a data object in the system) without the other."* and
//! *"Relationships where one data object 'contains' one or more other data
//! objects (i.e., a by-value relationship)"*; secondary: *"Both ends of the
//! relationships are optional."* The XSD rule is never applied to RESQML: it
//! would call `RepresentedInterpretation` secondary where the specification
//! writes "all relationships are primary".
//!
//! EML and PRODML follow the rule WITSML follows, and an edge no rule decides is
//! primary by default: the maintainer's decision of 23/10/2026, since ETP 1.2
//! leaves the rule to ML implementation guides that are not published for them
//! (note 1 of §8.1.1.1.2). Why primary and not secondary is written at
//! [`KindBasis::Default`]. ETP 1.2 §23.34.13 requires every `Edge` to carry
//! `Primary` or `Secondary`, so no edge judged here is left without a kind.
//!
//! The facts the XSD rule reads (whether a DOR slot is mandatory) are generated
//! (`energyml_core::relations`); the policy is here, written once, cited. A
//! package whose reference slots the generator does not compute (EML, PRODML
//! and RESQML today: their tables say "not computed") holds no fact, so its DORs
//! are primary by default until it does. `TODO(verify)`: the implementation
//! guides of WITSML, EML and PRODML for ETP 1.2, when published, replace the
//! derived and default rows.

use std::collections::HashMap;
use std::sync::Arc;

use energyml_core::identifiers::EtpUri;
use energyml_core::registry::Registry;

use crate::graph::{Edge, EdgeKind};

/// Where the kind of an edge comes from.
///
/// An edge whose basis is anything but [`KindBasis::EtpSpec`] or
/// [`KindBasis::Stated`] is this crate's inference, and says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindBasis {
    /// A server stated it (ETP `Edge.relationshipKind`).
    Stated,
    /// A rule the ETP specification writes.
    EtpSpec {
        /// The section, `8.2.2 (rule 1.a)`.
        section: &'static str,
        /// The page of ETP 1.2.
        page: u16,
    },
    /// By-value means primary: a "common characteristic" of a primary
    /// relationship (ETP 1.2 §8.1.1.1.2, p. 157), not a definition.
    DerivedByValue,
    /// The XSD makes the slot mandatory (primary) or optional (secondary):
    /// "one end mandatory" against "both ends optional" (§8.1.1.1.2, p. 157).
    DerivedXsd {
        /// The effective `minOccurs >= 1` of the DOR slot.
        required: bool,
    },
    /// No rule applies and the schema tables hold no fact: **primary by
    /// default**, the maintainer's decision of 23/10/2026, neither a rule of the
    /// specification nor an XSD fact.
    ///
    /// Why primary: discovery follows primary edges by default (ETP
    /// `navigableEdges`), so a primary edge called secondary would
    /// silently hide objects from a discovery, while the opposite error shows one
    /// object too many, which is visible; and RESQML's convention is all primary
    /// (§8.2.2 rule 1.a, p. 167-168).
    Default {
        /// Why no rule applies, for a reader who asks where the kind comes from.
        reason: &'static str,
    },
    /// No rule, no kind: the edge is refused by name, never guessed.
    ///
    /// [`StandardRules`] and [`judge_edges`] no longer answer it (every edge
    /// they judge has a kind, [`KindBasis::Default`] at worst); it is kept for
    /// a [`KindRules`] of a caller's that has no answer to give.
    Unknown {
        /// Why no rule applies, for the refusal.
        why: &'static str,
    },
}

impl KindBasis {
    /// Whether this is a rule the specification states (or a server did), as
    /// opposed to this crate's derivation, its default or no rule at all.
    pub fn is_official(&self) -> bool {
        matches!(self, KindBasis::Stated | KindBasis::EtpSpec { .. })
    }

    /// Whether the kind is this crate's default ([`KindBasis::Default`]): no
    /// rule applied and no schema fact was found.
    pub fn is_default(&self) -> bool {
        matches!(self, KindBasis::Default { .. })
    }
}

/// One judgement: a kind, and what it rests on.
///
/// `kind` is `None` exactly when `basis` is [`KindBasis::Unknown`], which
/// [`StandardRules`] never answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judged {
    /// The kind, or `None` when no rule applies.
    pub kind: Option<EdgeKind>,
    /// Where it comes from.
    pub basis: KindBasis,
}

impl Judged {
    /// A primary relationship on this basis.
    pub fn primary(basis: KindBasis) -> Self {
        Self {
            kind: Some(EdgeKind::Primary),
            basis,
        }
    }

    /// A secondary relationship on this basis.
    pub fn secondary(basis: KindBasis) -> Self {
        Self {
            kind: Some(EdgeKind::Secondary),
            basis,
        }
    }

    /// No rule applies and no fact is held: primary, by default
    /// ([`KindBasis::Default`]), saying why.
    pub fn by_default(reason: &'static str) -> Self {
        Self::primary(KindBasis::Default { reason })
    }

    /// No rule applies, and no kind is given.
    pub fn unknown(why: &'static str) -> Self {
        Self {
            kind: None,
            basis: KindBasis::Unknown { why },
        }
    }

    /// Whether `kind` and `basis` agree: no kind exactly when unknown.
    pub fn is_consistent(&self) -> bool {
        self.kind.is_none() == matches!(self.basis, KindBasis::Unknown { .. })
    }
}

/// The rules that say which relationships of an ML are primary.
pub trait KindRules: Send + Sync {
    /// The kind of a DOR written at `path` (list indices `[n]` removed or not)
    /// in an object of this type: the ML of the object holding the DOR decides
    /// (ETP 1.2 §8.1.1.1.2, p. 157; §8.2.2 rule 1, p. 167).
    fn dor(&self, source_qualified_type: &str, path: &str) -> Judged;

    /// The kind of the by-value relationship between an object and the
    /// container of this type that holds it: the ML of the container decides
    /// (§8.1.1.1.2, p. 157).
    fn containment(&self, container_qualified_type: &str) -> Judged;
}

/// The ML a qualified type belongs to, as far as the policy distinguishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ml {
    Resqml,
    Witsml,
    Prodml,
    Eml,
    Other,
}

impl Ml {
    /// From a domain (`resqml`) and a compact version (`22`).
    fn of(domain: &str, version: &str) -> Self {
        match (domain, version) {
            ("resqml", "20" | "22") => Ml::Resqml,
            ("witsml", "20" | "21") => Ml::Witsml,
            ("prodml", _) => Ml::Prodml,
            ("eml", _) => Ml::Eml,
            _ => Ml::Other,
        }
    }

    /// From a qualified type, `witsml21.Log`; the type name is returned with it.
    fn of_type(qualified_type: &str) -> (Self, &str) {
        match energyml_core::identity::parse_qualified_type(qualified_type) {
            Ok(parts) => (Ml::of(parts.domain, parts.version), parts.object_type),
            Err(_) => (Ml::Other, ""),
        }
    }
}

/// ETP 1.2 §8.2.2 rule 1.a, p. 167-168: "In RESQML, all relationships are primary".
const RESQML_ALL_PRIMARY: KindBasis = KindBasis::EtpSpec {
    section: "8.2.2 (rule 1.a)",
    page: 167,
};

/// ETP 1.2 §8.1.1.1.2, p. 157: the examples of the specification.
const WITSML_EXAMPLE: KindBasis = KindBasis::EtpSpec {
    section: "8.1.1.1.2",
    page: 157,
};

/// What the XSD fact lookup answers: whether the DOR slot of a type at a path is
/// mandatory, or `None` when there is no fact (or the facts disagree).
type RequiredLookup = dyn Fn(&str, &str) -> Option<bool> + Send + Sync;

/// The policy of this crate: RESQML and the WITSML examples as the specification
/// states them, WITSML, EML and PRODML otherwise by the XSD rule and by-value
/// containment (labelled derived), and primary by default where no rule applies
/// and no fact is held (labelled [`KindBasis::Default`]).
///
/// The XSD facts come from the generated relation tables of the registry
/// ([`StandardRules::over`]) or from a lookup the caller supplies
/// ([`StandardRules::with_required`]).
pub struct StandardRules {
    required: Option<Box<RequiredLookup>>,
}

impl StandardRules {
    /// The policy with **no XSD facts**: a WITSML, EML or PRODML DOR outside
    /// the examples of the specification is primary by default.
    pub fn new() -> Self {
        Self { required: None }
    }

    /// The policy over a lookup of "is this DOR slot mandatory": called with the
    /// qualified type of the holder and the path of the DOR (indices removed),
    /// answering `None` when it has no fact.
    pub fn with_required(lookup: impl Fn(&str, &str) -> Option<bool> + Send + Sync + 'static) -> Self {
        Self {
            required: Some(Box::new(lookup)),
        }
    }

    /// The policy over the generated tables of a registry: a DOR slot is
    /// mandatory by `DorSlot::required` (the effective `minOccurs` of the XSD),
    /// found by its path in `TypeRelations::refers`.
    ///
    /// A path that two slots share with different answers (a polymorphic
    /// element) has no fact: it is primary by default rather than decided.
    pub fn over(registry: Arc<Registry>) -> Self {
        Self::with_required(move |qualified_type, path| {
            let info = registry.by_qualified_type(qualified_type)?.info;
            let relations = registry.relations_of(info)?;
            let mut answer = None;
            for slot in relations.refers {
                let slot_path = slot
                    .path
                    .iter()
                    .map(|step| step.name)
                    .collect::<Vec<_>>()
                    .join(".");
                if slot_path != path {
                    continue;
                }
                match answer {
                    None => answer = Some(slot.required),
                    Some(seen) if seen == slot.required => {}
                    Some(_) => return None,
                }
            }
            answer
        })
    }

    /// The ML prefixes (`eml23`, `resqml22`, `witsml21`) of the registry's types
    /// for which this policy states a kind for **some** edge. Every edge has a
    /// kind under this policy ([`KindBasis::Default`] at worst), so that is every
    /// prefix of the registry; a store judging its edges by it reports
    /// `EdgeKindCoverage::All`, which also covers types the registry lacks.
    /// Sorted, without duplicates.
    pub fn covered_prefixes(&self, registry: &Registry) -> Vec<String> {
        let mut prefixes: Vec<String> = registry
            .types()
            .map(|info| format!("{}{}", info.package, info.schema_version.replace('.', "")))
            .collect();
        prefixes.sort();
        prefixes.dedup();
        prefixes
    }
}

impl Default for StandardRules {
    fn default() -> Self {
        Self::new()
    }
}

/// `Grid2dPatch[0].Points[3].X` to `Grid2dPatch.Points.X`.
fn without_indices(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut skipping = false;
    for c in path.chars() {
        match c {
            '[' => skipping = true,
            ']' => skipping = false,
            _ if !skipping => out.push(c),
            _ => {}
        }
    }
    out
}

/// The reason of a DOR whose slot the schema tables hold no fact for.
const NO_XSD_FACT: &str = "no rule of the specification and no schema fact for this reference: primary by default (maintainer decision of 23/10/2026; ETP 1.2 section 8.1.1.1.2 note 1: the implementation guide is not published)";

impl StandardRules {
    /// ETP 1.2 §8.1.1.1.2 (p. 157): primary when "one end of the relationship
    /// is mandatory; that is, one object cannot exist (as a data object in the
    /// system) without the other", secondary when "both ends of the
    /// relationships are optional"; read on the XSD of the holder, whose ML
    /// decides ("the type of relationship is determined by the ML of the data
    /// object that has the DOR"). No fact: primary by default.
    fn by_xsd(&self, source_qualified_type: &str, path: &str) -> Judged {
        match self
            .required
            .as_ref()
            .and_then(|lookup| lookup(source_qualified_type, path))
        {
            Some(true) => Judged::primary(KindBasis::DerivedXsd { required: true }),
            Some(false) => Judged::secondary(KindBasis::DerivedXsd { required: false }),
            None => Judged::by_default(NO_XSD_FACT),
        }
    }
}

impl KindRules for StandardRules {
    fn dor(&self, source_qualified_type: &str, path: &str) -> Judged {
        let (ml, type_name) = Ml::of_type(source_qualified_type);
        match ml {
            // ETP 1.2 §8.2.2 rule 1.a (p. 167-168). The holder's ML decides, even
            // when the target is WITSML.
            Ml::Resqml => Judged::primary(RESQML_ALL_PRIMARY),
            Ml::Witsml => {
                let path = without_indices(path);
                // ETP 1.2 §8.1.1.1.2 (p. 157): "the reference from a Channel to a
                // Wellbore" is the specification's example of a secondary one.
                if type_name == "Channel" && path == "Wellbore" {
                    return Judged::secondary(WITSML_EXAMPLE);
                }
                self.by_xsd(source_qualified_type, &path)
            }
            // The rule WITSML follows, by the maintainer's decision of 23/10/2026:
            // no implementation guide states one for EML or PRODML.
            Ml::Prodml | Ml::Eml => self.by_xsd(source_qualified_type, &without_indices(path)),
            Ml::Other => Judged::by_default(
                "no rule for the ML of this object: primary by default (maintainer decision of 23/10/2026; ETP 1.2 section 8.1.1.1.2)",
            ),
        }
    }

    fn containment(&self, container_qualified_type: &str) -> Judged {
        let (ml, type_name) = Ml::of_type(container_qualified_type);
        match ml {
            Ml::Resqml => Judged::primary(RESQML_ALL_PRIMARY),
            Ml::Witsml => match type_name {
                // ETP 1.2 §8.1.1.1.2 (p. 157): "organizing Channels into ChannelSets
                // or organizing ChannelSets into Logs".
                "Log" | "ChannelSet" => Judged::primary(WITSML_EXAMPLE),
                _ => Judged::primary(KindBasis::DerivedByValue),
            },
            // ETP 1.2 §8.1.1.1.2 (p. 157): "Relationships where one data object
            // 'contains' one or more other data objects (i.e., a by-value
            // relationship)" are primary; the container's ML decides. EML and
            // PRODML by the maintainer's decision of 23/10/2026.
            Ml::Prodml | Ml::Eml => Judged::primary(KindBasis::DerivedByValue),
            Ml::Other => Judged::by_default(
                "no rule for the ML of this container: primary by default (maintainer decision of 23/10/2026; ETP 1.2 section 8.1.1.1.2)",
            ),
        }
    }
}

/// The qualified type of an object URI (`resqml22.Grid2dRepresentation`), read
/// by the one door for URIs.
pub(crate) fn qualified_type_of(uri: &str) -> Option<String> {
    let parsed = EtpUri::parse(uri).ok()?;
    Some(format!(
        "{}{}.{}",
        parsed.domain?, parsed.version?, parsed.object_type?
    ))
}

/// Give every edge its kind and the basis of it, as `rules` judge it: by the
/// qualified type of the edge's source (`Edge::from`) and its path.
///
/// An edge that already states its kind keeps it. A source URI that cannot be
/// read has no ML to decide: primary by default ([`KindBasis::Default`], which
/// says so). The source's type is read once per distinct source.
pub fn judge_edges(rules: &dyn KindRules, edges: Vec<Edge>) -> Vec<Edge> {
    let mut types: HashMap<String, Option<String>> = HashMap::new();
    edges
        .into_iter()
        .map(|edge| {
            if edge.kind.is_some() {
                return edge;
            }
            let source = types
                .entry(edge.from.clone())
                .or_insert_with(|| qualified_type_of(&edge.from));
            let judged = match source {
                Some(qualified_type) => rules.dor(qualified_type, &edge.path),
                None => Judged::by_default(
                    "the source of the edge is not an object URI, so no ML decides: primary by default (maintainer decision of 23/10/2026)",
                ),
            };
            edge.with_judgement(judged)
        })
        .collect()
}
