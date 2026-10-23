//! The kind of an edge, primary or secondary: contract rules KIND-1 to KIND-5.
//!
//! Every rule is written from the ETP 1.2 specification (Energistics ETP
//! v1.2, section and page quoted at each test), or from the maintainer's
//! decision of 23/10/2026 (EML and PRODML follow the WITSML rule; an edge no
//! rule decides is primary by default), over edges built by hand: no package is
//! read, since the corpus does not decide what the specification says. The
//! facts a rule reads (is this DOR slot mandatory in the XSD) are supplied by a
//! lookup the test writes, or read from the generated code (the relation tables
//! and the reflection of the generated types, both from the XSD).
//!
//! Each test says how to break it: change the policy, run the test, it must
//! fail. Broken on purpose when the default and the EML and PRODML rows were
//! written (23/10/2026), each break restored after:
//!
//! - EML and PRODML made unknown again in `StandardRules::dor` and
//!   `containment` (the arms as they were before): six of the nine kind tests
//!   fell, `kind_3_a_derived_kind_says_so` (its sweep wants a kind on every
//!   edge), both `kind_4_*` (the EML and PRODML edges have none) and the three
//!   `kind_5_*`; KIND-1, KIND-2 and the RESQML discovery test held.
//! - The default made secondary (`Judged::by_default` answering
//!   `Judged::secondary`): `kind_3_a_derived_kind_says_so`,
//!   `kind_4_an_edge_no_rule_decides_is_primary_by_default`,
//!   `kind_4_the_refusal_is_met_not_global` (the PRODML walk finds nothing) and
//!   `kind_5_the_registry_facts_decide_eml_and_prodml_too` fell.
//! - The default labelled as an XSD fact (`DerivedXsd { required: true }`
//!   in place of `Default` in `by_xsd`): `kind_3_a_derived_kind_says_so`,
//!   `kind_4_an_edge_no_rule_decides_is_primary_by_default` and
//!   `kind_5_the_registry_facts_decide_eml_and_prodml_too` fell.

use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::context::discover;
use energyml_storage::kinds::{Judged, KindBasis, KindRules, StandardRules, judge_edges};
use energyml_storage::{Context, Edge, EdgeKind, Error, Navigable, ObjectSummary, Scope};

const GRID: &str = "eml:///resqml22.Grid2dRepresentation(7e000000-0000-4000-8000-000000000001)";
const HORIZON: &str = "eml:///resqml22.HorizonInterpretation(7e000000-0000-4000-8000-000000000002)";
const FEATURE_20: &str = "eml:///resqml20.obj_WellboreFeature(7e000000-0000-4000-8000-000000000003)";
const WELLBORE_20: &str = "eml:///witsml20.Wellbore(7e000000-0000-4000-8000-000000000004)";
const CHANNEL_20: &str = "eml:///witsml20.Channel(7e000000-0000-4000-8000-000000000005)";
const CHANNEL_21: &str = "eml:///witsml21.Channel(7e000000-0000-4000-8000-000000000006)";
const WELLBORE_21: &str = "eml:///witsml21.Wellbore(7e000000-0000-4000-8000-000000000007)";
const LOG_20: &str = "eml:///witsml20.Log(7e000000-0000-4000-8000-000000000008)";
const LOG_21: &str = "eml:///witsml21.Log(7e000000-0000-4000-8000-000000000009)";
const FLOW_TEST: &str = "eml:///prodml23.FlowTestActivity(7e000000-0000-4000-8000-00000000000a)";
const CRS: &str = "eml:///eml23.LocalEngineeringCompoundCrs(7e000000-0000-4000-8000-00000000000b)";
const ACTIVITY: &str = "eml:///eml23.Activity(7e000000-0000-4000-8000-00000000000c)";
const TEMPLATE: &str = "eml:///eml23.ActivityTemplate(7e000000-0000-4000-8000-00000000000d)";
const PROPERTY_KIND: &str = "eml:///eml23.PropertyKind(7e000000-0000-4000-8000-00000000000e)";
const PARENT_KIND: &str = "eml:///eml23.PropertyKind(7e000000-0000-4000-8000-00000000000f)";
const DAS: &str = "eml:///prodml23.DasAcquisition(7e000000-0000-4000-8000-000000000010)";
const OPTICAL_PATH: &str = "eml:///prodml23.FiberOpticalPath(7e000000-0000-4000-8000-000000000011)";
const ASSOCIATE: &str = "eml:///eml23.BusinessAssociate(7e000000-0000-4000-8000-000000000012)";

/// A policy whose XSD lookup says `required` for every WITSML slot it is asked
/// about.
fn rules_where_xsd_says(required: bool) -> StandardRules {
    StandardRules::with_required(move |_, _| Some(required))
}

fn judged(rules: &StandardRules, from: &str, to: &str, path: &str) -> Edge {
    judge_edges(rules, vec![Edge::new(from, to, path)])
        .pop()
        .expect("one edge in, one edge out")
}

/// Every schema version the workspace generates.
fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// A lookup over the **reflection of the generated types**: a one-step path
/// names a field of the holder; it is a DOR slot when the field's type is a
/// data object reference, mandatory when its XSD `minOccurs` is at least one.
/// The cardinality comes from the generated code, not from the test.
fn reflected(registry: Arc<Registry>) -> StandardRules {
    StandardRules::with_required(move |qualified_type, path| reflected_slot(&registry, qualified_type, path))
}

/// Whether the reflection of the generated types says this DOR slot is
/// mandatory (`Some(true)`), optional (`Some(false)`), or is no DOR slot.
fn reflected_slot(registry: &Registry, qualified_type: &str, path: &str) -> Option<bool> {
    let field = registry.by_qualified_type(qualified_type)?.info.field(path)?;
    field.ty.type_info().filter(|info| info.is_object_ref)?;
    Some(field.occurs.min >= 1)
}

fn summary(uri: &str) -> ObjectSummary {
    let open = uri.find('(').expect("a uri with a uuid");
    let qualified = &uri["eml:///".len()..open];
    let uuid = &uri[open + 1..uri.len() - 1];
    ObjectSummary::new(uri, uuid, qualified)
}

// ---------------------------------------------------------------------------
// KIND-1
// ---------------------------------------------------------------------------

/// KIND-1. ETP v1.2 section 8.2.2, rule 1.a (p. 167-168): "In RESQML, all
/// relationships are primary, so when a RESQML wellbore feature references a
/// WITSML wellbore, the navigableEdge (relationship) is primary." The ML of the
/// object holding the DOR decides (section 8.1.1.1.2, p. 157), whatever the
/// target is.
///
/// To break it: make `StandardRules::dor` answer `Secondary` for `Ml::Resqml`,
/// or decide by the ML of the target; both assertions fail.
#[test]
fn kind_1_every_resqml_edge_is_primary() {
    // Even when the XSD would say "optional": the XSD rule is never applied to RESQML.
    let rules = rules_where_xsd_says(false);
    for (from, to, path) in [
        (GRID, HORIZON, "RepresentedInterpretation"),
        (GRID, CRS, "LocalCrs"),
        (FEATURE_20, WELLBORE_20, "WitsmlWellbore"),
    ] {
        let edge = judged(&rules, from, to, path);
        assert_eq!(edge.kind, Some(EdgeKind::Primary), "{from} -> {to}");
        assert_eq!(
            edge.basis,
            Some(KindBasis::EtpSpec {
                section: "8.2.2 (rule 1.a)",
                page: 167
            }),
            "{from} -> {to}"
        );
    }
    // Index and the by-value side of RESQML (a colour map in its dictionary) say the same.
    assert_eq!(
        rules
            .dor("resqml22.Grid2dRepresentation", "Patches[3].Geometry")
            .kind,
        Some(EdgeKind::Primary)
    );
    assert_eq!(
        rules.containment("resqml22.ColorMapDictionary").kind,
        Some(EdgeKind::Primary)
    );
}

// ---------------------------------------------------------------------------
// KIND-2
// ---------------------------------------------------------------------------

/// KIND-2. ETP v1.2 section 8.1.1.1.2 (p. 157): primary relationships may
/// "organize or group data objects, such as organizing Channels into ChannelSets
/// or organizing ChannelSets into Logs"; a secondary one is "the reference from a
/// Channel to a Wellbore". Both WITSML 2.0 and 2.1.
///
/// The XSD lookup is rigged to say the opposite of the specification for the
/// Channel's wellbore (mandatory); the specification's example must win.
///
/// To break it: remove the `Channel`/`Wellbore` example from `StandardRules::dor`
/// (the XSD rule then answers primary), or the `Log`/`ChannelSet` arm of
/// `containment` (it then answers `DerivedByValue`).
#[test]
fn kind_2_the_specifications_witsml_examples() {
    let rules = rules_where_xsd_says(true);
    let official = KindBasis::EtpSpec {
        section: "8.1.1.1.2",
        page: 157,
    };
    for (channel, wellbore) in [(CHANNEL_20, WELLBORE_20), (CHANNEL_21, WELLBORE_21)] {
        let edge = judged(&rules, channel, wellbore, "Wellbore");
        assert_eq!(edge.kind, Some(EdgeKind::Secondary), "{channel}");
        assert_eq!(edge.basis, Some(official), "{channel}");
    }
    for version in ["witsml20", "witsml21"] {
        for container in ["Log", "ChannelSet"] {
            let judged = rules.containment(&format!("{version}.{container}"));
            assert_eq!(judged.kind, Some(EdgeKind::Primary), "{version}.{container}");
            assert_eq!(judged.basis, official, "{version}.{container}");
        }
    }
}

// ---------------------------------------------------------------------------
// KIND-3
// ---------------------------------------------------------------------------

/// KIND-3. ETP v1.2 section 8.1.1.1.2 note 1 (p. 157): the ML rules are in the
/// ML implementation guides, which say nothing here. What this crate derives
/// from the "common characteristics" of p. 157 (one end mandatory is primary,
/// both ends optional is secondary, by-value is primary) is labelled as derived,
/// and **never** as the specification's rule. The only official answers are
/// RESQML's (section 8.2.2) and the three WITSML examples.
///
/// To break it: label a derived answer `EtpSpec` in `StandardRules`; the sweep
/// finds an official answer outside the five.
#[test]
fn kind_3_a_derived_kind_says_so() {
    // The XSD facts differ by type, so the same path can differ by version: the
    // answer follows the type of the holder (`Log.Wellbore` is mandatory in
    // WITSML 2.0 and optional in 2.1, Log.xsd).
    let rules = StandardRules::with_required(|qualified_type, path| match (qualified_type, path) {
        ("witsml20.Log", "Wellbore") => Some(true),
        ("witsml21.Log", "Wellbore") => Some(false),
        _ => None,
    });
    let mandatory = judged(&rules, LOG_20, WELLBORE_20, "Wellbore");
    assert_eq!(mandatory.kind, Some(EdgeKind::Primary));
    assert_eq!(mandatory.basis, Some(KindBasis::DerivedXsd { required: true }));
    let optional = judged(&rules, LOG_21, WELLBORE_21, "Wellbore");
    assert_eq!(optional.kind, Some(EdgeKind::Secondary));
    assert_eq!(optional.basis, Some(KindBasis::DerivedXsd { required: false }));

    // Another by-value relationship of WITSML: primary, derived from "ByValue".
    let by_value = rules.containment("witsml21.WellboreMarkerSet");
    assert_eq!(by_value.kind, Some(EdgeKind::Primary));
    assert_eq!(by_value.basis, KindBasis::DerivedByValue);

    // A WITSML DOR the tables know nothing about is primary by default, and
    // says it is a default, not an XSD fact (the maintainer's decision of
    // 23/10/2026).
    let no_fact = judged(&rules, LOG_21, WELLBORE_21, "SomethingElse");
    assert_eq!(no_fact.kind, Some(EdgeKind::Primary));
    assert!(
        matches!(no_fact.basis, Some(KindBasis::Default { .. })),
        "{:?}",
        no_fact.basis
    );

    // The sweep: over every ML, every path, both sides, the official answers are
    // RESQML's and the three examples, nothing else; and every answer has a
    // kind (ETP 1.2 section 23.34.13: an `Edge` is `Primary` or `Secondary`).
    let types = [
        "resqml20.obj_Grid2dRepresentation",
        "resqml22.Grid2dRepresentation",
        "witsml20.Log",
        "witsml20.ChannelSet",
        "witsml20.Channel",
        "witsml21.Log",
        "witsml21.ChannelSet",
        "witsml21.Channel",
        "witsml21.WellboreMarkerSet",
        "prodml23.FlowTestActivity",
        "eml23.LocalEngineeringCompoundCrs",
        "other9.Thing",
    ];
    let lenient = rules_where_xsd_says(true);
    let mut official: Vec<String> = Vec::new();
    for qualified_type in types {
        for path in ["Wellbore", "Citation.Creator", "Other"] {
            let judged = lenient.dor(qualified_type, path);
            assert!(judged.is_consistent(), "{qualified_type} {path}: {judged:?}");
            assert!(judged.kind.is_some(), "{qualified_type} {path}: {judged:?}");
            if matches!(judged.basis, KindBasis::EtpSpec { .. }) {
                official.push(format!("dor {qualified_type} {path}"));
            }
        }
        let judged = lenient.containment(qualified_type);
        assert!(judged.is_consistent(), "{qualified_type}: {judged:?}");
        assert!(judged.kind.is_some(), "{qualified_type}: {judged:?}");
        if matches!(judged.basis, KindBasis::EtpSpec { .. }) {
            official.push(format!("containment {qualified_type}"));
        }
    }
    let allowed = |entry: &str| {
        // RESQML: any of its edges (section 8.2.2).
        entry.contains(" resqml2")
            // WITSML examples (section 8.1.1.1.2, p. 157).
            || (entry.starts_with("dor witsml2") && entry.contains(".Channel Wellbore"))
            || (entry.starts_with("containment witsml2")
                && (entry.ends_with(".Log") || entry.ends_with(".ChannelSet")))
    };
    let stray: Vec<&String> = official.iter().filter(|entry| !allowed(entry)).collect();
    assert!(
        stray.is_empty(),
        "official without a text of the specification: {stray:?}"
    );
    // Not vacuous: the three examples in each of the two WITSML versions (the
    // Channel to Wellbore reference, Log and ChannelSet as containers) were seen.
    assert_eq!(
        official.iter().filter(|e| e.contains("witsml")).count(),
        2 * 3,
        "{official:?}"
    );
    assert!(official.iter().any(|e| e.contains("resqml")), "{official:?}");
}

// ---------------------------------------------------------------------------
// KIND-4
// ---------------------------------------------------------------------------

/// KIND-4, the policy half. ETP 1.2 section 23.34.13 requires every `Edge` to
/// be `Primary` or `Secondary`; where no rule applies and the schema tables hold
/// no fact, the edge is **primary by default** (the maintainer's decision of
/// 23/10/2026: discovery follows primary edges by default, so the error that
/// hides objects is the one to avoid), labelled [`KindBasis::Default`] and
/// never as a rule of the specification or an XSD fact. That covers an ML with
/// no rule (another WITSML version, an unknown ML), a source that is not an
/// object URI, and a WITSML, EML or PRODML DOR with no fact.
///
/// To break it: make `Judged::by_default` answer `Secondary`, or label it
/// `DerivedXsd`; the assertions on `kind` or on `basis` fail.
#[test]
fn kind_4_an_edge_no_rule_decides_is_primary_by_default() {
    let is_default = |basis: Option<KindBasis>| match basis {
        Some(KindBasis::Default { reason }) => !reason.is_empty(),
        _ => false,
    };
    // No XSD facts at all: no DOR of WITSML, EML or PRODML outside the
    // specification's examples has one.
    let no_facts = StandardRules::new();
    for (from, to, path) in [
        (FLOW_TEST, CHANNEL_21, "Anything"),
        (CRS, GRID, "Anything"),
        (LOG_21, WELLBORE_21, "Wellbore"),
        (
            "eml:///other9.Thing(7e000000-0000-4000-8000-000000000013)",
            GRID,
            "Anything",
        ),
        (
            "eml:///witsml14.well(7e000000-0000-4000-8000-000000000014)",
            GRID,
            "Anything",
        ),
    ] {
        let edge = judged(&no_facts, from, to, path);
        assert_eq!(edge.kind, Some(EdgeKind::Primary), "{from}");
        assert!(is_default(edge.basis), "{from}: {:?}", edge.basis);
        assert!(!edge.basis.unwrap().is_official(), "{from}");
    }
    // An ML with no rule as a container.
    for container in ["witsml14.well", "other9.Thing", "nonsense"] {
        let judged = no_facts.containment(container);
        assert_eq!(judged.kind, Some(EdgeKind::Primary), "{container}");
        assert!(is_default(Some(judged.basis)), "{container}: {judged:?}");
    }
    // A source that is not an object URI: no ML decides, primary by default.
    let edge = judge_edges(&no_facts, vec![Edge::new("not a uri", GRID, "x")]).remove(0);
    assert_eq!(edge.kind, Some(EdgeKind::Primary));
    assert!(is_default(edge.basis), "{:?}", edge.basis);
    // The default never covers a stated rule: RESQML stays the specification's.
    let resqml = judged(&no_facts, GRID, HORIZON, "RepresentedInterpretation");
    assert!(matches!(resqml.basis, Some(KindBasis::EtpSpec { .. })));
}

/// Rules of a caller's that know no kind for PRODML: what an unjudged edge
/// looks like now that the standard rules give every edge one.
struct NoProdml;

impl KindRules for NoProdml {
    fn dor(&self, source_qualified_type: &str, path: &str) -> Judged {
        if source_qualified_type.starts_with("prodml") {
            return Judged::unknown("PRODML: this caller's rules state no kind");
        }
        StandardRules::new().dor(source_qualified_type, path)
    }

    fn containment(&self, container_qualified_type: &str) -> Judged {
        StandardRules::new().containment(container_qualified_type)
    }
}

/// KIND-4, the discovery half. The standard rules give the PRODML edge a kind
/// (primary by default: the maintainer's decision of 23/10/2026), so a walk by
/// kind from it answers. An edge that still has none (a caller's rules that
/// know none) is refused where it is **met**, not globally: a request by kind
/// answers while the walk touches only edges that have a kind, and refuses by
/// name, with the type and the cause, at the first edge that has none (ETP
/// v1.2 section 8.1.1.1.4 steps 1 and 2, p. 158).
///
/// To break it: make the default secondary; the first walk finds nothing.
/// Restore the global test in `discover` (refuse when any edge of the dataspace
/// has no kind); the second assertion fails. Or drop the check in the walk; the
/// third fails.
#[test]
fn kind_4_the_refusal_is_met_not_global() {
    let resources: Vec<ObjectSummary> = [GRID, HORIZON, FLOW_TEST, CHANNEL_21].map(summary).to_vec();
    let written = vec![
        Edge::new(GRID, HORIZON, "RepresentedInterpretation"),
        Edge::new(FLOW_TEST, CHANNEL_21, "ChannelSet"),
    ];

    // The standard rules, with no XSD fact: the PRODML edge is primary by
    // default, followed.
    let standard = judge_edges(&StandardRules::new(), written.clone());
    let mut from_prodml = Context::of(FLOW_TEST, Scope::Targets);
    from_prodml.navigable = Navigable::Primary;
    let found = discover("eml:///", resources.clone(), standard, &from_prodml)
        .expect("every edge the standard rules judge has a kind");
    let reached: Vec<&str> = found.resources.iter().map(|r| r.uri.as_str()).collect();
    assert_eq!(reached, vec![CHANNEL_21]);

    // Rules that leave the PRODML edge without a kind.
    let edges = judge_edges(&NoProdml, written);
    let mut context = Context::of(GRID, Scope::Targets);
    context.navigable = Navigable::Primary;

    // The walk from the RESQML object never touches the PRODML edge: it answers.
    let found = discover("eml:///", resources.clone(), edges.clone(), &context)
        .expect("a walk that meets only edges with a kind answers");
    let mut reached: Vec<&str> = found.resources.iter().map(|r| r.uri.as_str()).collect();
    reached.sort_unstable();
    assert_eq!(reached, vec![HORIZON]);

    // The walk that reaches the PRODML edge refuses, naming the type and the cause.
    match discover("eml:///", resources.clone(), edges.clone(), &from_prodml) {
        Err(Error::UnstatedKind { source_type, why }) => {
            assert_eq!(source_type, "prodml23.FlowTestActivity");
            assert!(why.contains("PRODML"), "{why}");
        }
        other => panic!("expected UnstatedKind, found {other:?}"),
    }

    // A request that needs no kind (both, no expansion) answers over the same edges.
    let both = Context::of(FLOW_TEST, Scope::Targets);
    assert!(discover("eml:///", resources, edges, &both).is_ok());
}

// ---------------------------------------------------------------------------
// KIND-5
// ---------------------------------------------------------------------------

/// KIND-5, DORs. EML and PRODML follow the rule WITSML follows (the
/// maintainer's decision of 23/10/2026), read in ETP v1.2 section 8.1.1.1.2
/// (p. 157): primary when "One end of the relationship is mandatory; that is,
/// one object cannot exist (as a data object in the system) without the
/// other.", secondary when "Both ends of the relationships are optional."; "the
/// type of relationship is determined by the ML of the data object that has the
/// DOR". The cardinality is read from the generated types (XSD `minOccurs`):
/// `eml23.Activity.ActivityDescriptor` is mandatory, `eml23.Activity.Parent`
/// and `eml23.PropertyKind.Parent` optional, `prodml23.DasAcquisition`
/// `OpticalPath` mandatory and `ServiceCompanyDetails` optional.
///
/// To break it: make EML or PRODML unknown again in `StandardRules::dor`, or
/// give them the default without reading the XSD; an assertion on `basis` fails.
#[test]
fn kind_5_eml_and_prodml_dors_follow_the_xsd() {
    let registry = registry();
    let rules = reflected(registry.clone());
    for (from, to, path, mandatory) in [
        (ACTIVITY, TEMPLATE, "ActivityDescriptor", true),
        (ACTIVITY, ACTIVITY, "Parent", false),
        (PROPERTY_KIND, PARENT_KIND, "Parent", false),
        (DAS, OPTICAL_PATH, "OpticalPath", true),
        (DAS, ASSOCIATE, "ServiceCompanyDetails", false),
    ] {
        let source = &from["eml:///".len()..from.find('(').expect("an object URI")];
        // Not vacuous: the generated code says what the case claims.
        assert_eq!(
            reflected_slot(&registry, source, path),
            Some(mandatory),
            "{source}.{path} in the generated types"
        );
        let edge = judged(&rules, from, to, path);
        let (kind, basis) = if mandatory {
            (EdgeKind::Primary, KindBasis::DerivedXsd { required: true })
        } else {
            (EdgeKind::Secondary, KindBasis::DerivedXsd { required: false })
        };
        assert_eq!(edge.kind, Some(kind), "{source}.{path}");
        assert_eq!(edge.basis, Some(basis), "{source}.{path}");
    }
    // The holder decides: a RESQML object's DOR stays primary on RESQML's rule
    // even when it names an EML object through an optional slot.
    let resqml = judged(&rules_where_xsd_says(false), GRID, CRS, "LocalCrs");
    assert!(matches!(resqml.basis, Some(KindBasis::EtpSpec { page: 167, .. })));
}

/// KIND-5, by value. ETP v1.2 section 8.1.1.1.2 (p. 157): "Relationships where
/// one data object 'contains' one or more other data objects (i.e., a by-value
/// relationship)" are primary; for EML and PRODML containers by the
/// maintainer's decision of 23/10/2026, labelled derived. The containers are
/// those the generated relation tables name.
///
/// To break it: make EML or PRODML containment unknown again, or label it
/// `Default`; the assertions fail.
#[test]
fn kind_5_eml_and_prodml_containers_by_value_are_primary() {
    let registry = registry();
    let rules = StandardRules::over(registry.clone());
    for container in [
        "eml23.Aggregate",
        "eml23.PropertyKindDictionary",
        "eml23.LocalEngineering2dCrs",
        "prodml23.AssetProductionVolumes",
        "prodml23.ChannelSet",
    ] {
        assert!(
            registry.is_container(container),
            "{container} in the relation tables"
        );
        let judged = rules.containment(container);
        assert_eq!(judged.kind, Some(EdgeKind::Primary), "{container}");
        assert_eq!(judged.basis, KindBasis::DerivedByValue, "{container}");
    }
}

/// KIND-5, over the registry. `StandardRules::over` reads the XSD facts from the
/// generated relation tables only: a DOR slot the table of the holder states is
/// `DerivedXsd` with the table's `required`, one it does not state is primary
/// by default (the maintainer's decision of 23/10/2026), for EML and PRODML as
/// for WITSML. The WITSML 2.1 slots are taken from its tables, one mandatory
/// and one optional, so the machinery is shown working on real facts.
///
/// To break it: make EML unknown again, or label the default `DerivedXsd`;
/// the EML assertions fail.
#[test]
fn kind_5_the_registry_facts_decide_eml_and_prodml_too() {
    let registry = registry();
    let rules = StandardRules::over(registry.clone());
    let fact = |qualified_type: &str, path: &str| -> Option<bool> {
        let slots: Vec<bool> = registry
            .relations_for(qualified_type)?
            .refers
            .iter()
            .filter(|slot| slot.dotted_path() == path)
            .map(|slot| slot.required)
            .collect();
        match slots.as_slice() {
            [] => None,
            [first, rest @ ..] if rest.iter().all(|r| r == first) => Some(*first),
            _ => None,
        }
    };
    let expect = |qualified_type: &str, path: &str| {
        let judged = rules.dor(qualified_type, path);
        match fact(qualified_type, path) {
            Some(required) => {
                assert_eq!(
                    judged.basis,
                    KindBasis::DerivedXsd { required },
                    "{qualified_type}.{path}"
                );
                let kind = if required {
                    EdgeKind::Primary
                } else {
                    EdgeKind::Secondary
                };
                assert_eq!(judged.kind, Some(kind), "{qualified_type}.{path}");
            }
            None => {
                assert!(
                    matches!(judged.basis, KindBasis::Default { .. }),
                    "{qualified_type}.{path}: {judged:?}"
                );
                assert_eq!(judged.kind, Some(EdgeKind::Primary), "{qualified_type}.{path}");
            }
        }
    };
    for (qualified_type, path) in [
        ("eml23.Activity", "ActivityDescriptor"),
        ("eml23.Activity", "Parent"),
        ("eml23.PropertyKind", "Parent"),
        ("prodml23.DasAcquisition", "OpticalPath"),
        ("prodml23.DasAcquisition", "ServiceCompanyDetails"),
    ] {
        expect(qualified_type, path);
    }
    // WITSML 2.1: one mandatory and one optional slot of its tables, each the
    // only slot of its path in its type.
    let mut seen = [false, false];
    for info in registry
        .types()
        .filter(|i| i.package == "witsml" && i.version == "2.1")
    {
        let Some(relations) = registry.relations_of(info) else {
            continue;
        };
        for slot in relations.refers {
            let path = slot.dotted_path();
            if relations.name == "Channel" && path == "Wellbore" {
                continue;
            }
            if fact(relations.qualified_type, &path).is_some() && !seen[usize::from(slot.required)] {
                seen[usize::from(slot.required)] = true;
                expect(relations.qualified_type, &path);
            }
        }
    }
    assert_eq!(
        seen,
        [true, true],
        "the WITSML 2.1 tables hold a mandatory and an optional slot"
    );
}
