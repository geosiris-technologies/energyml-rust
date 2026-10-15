//! The kind of an edge, primary or secondary: contract rules KIND-1 to KIND-4.
//!
//! Every rule is written from the ETP 1.2 specification (Energistics ETP
//! v1.2, section and page quoted at each test) over edges built by hand: no
//! package is read, since the corpus does not decide what the specification says.
//! The facts a rule reads (is this DOR slot mandatory in the XSD) are supplied
//! by a lookup the test writes.
//!
//! Each test says how to break it: change the policy, run the test, it must
//! fail.

use energyml_storage::context::discover;
use energyml_storage::kinds::{KindBasis, KindRules, StandardRules, judge_edges};
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

    // A WITSML DOR the tables know nothing about is not guessed.
    let unknown = judged(&rules, LOG_21, WELLBORE_21, "SomethingElse");
    assert_eq!(unknown.kind, None);
    assert!(matches!(unknown.basis, Some(KindBasis::Unknown { .. })));

    // The sweep: over every ML, every path, both sides, the official answers are
    // RESQML's and the three examples, nothing else.
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
            if matches!(judged.basis, KindBasis::EtpSpec { .. }) {
                official.push(format!("dor {qualified_type} {path}"));
            }
        }
        let judged = lenient.containment(qualified_type);
        assert!(judged.is_consistent(), "{qualified_type}: {judged:?}");
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

/// KIND-4, the policy half. ETP v1.2 section 8.1.1.1.2 note 1 (p. 157): the
/// guide of an ML that has none says nothing, so PRODML, EML alone and an unknown
/// ML have no kind, and say why. WITSML 2.0 and 2.1 only; another WITSML version
/// is no rule either.
///
/// To break it: give PRODML a kind (any, in `StandardRules::dor`); the
/// assertion on `kind` fails.
#[test]
fn kind_4_an_ml_without_a_rule_has_no_kind_and_says_why() {
    let rules = rules_where_xsd_says(true);
    for (from, to) in [(FLOW_TEST, CHANNEL_21), (CRS, GRID)] {
        let edge = judged(&rules, from, to, "Anything");
        assert_eq!(edge.kind, None, "{from}");
        match edge.basis {
            Some(KindBasis::Unknown { why }) => assert!(!why.is_empty(), "{from}"),
            other => panic!("{from}: expected an unknown basis, found {other:?}"),
        }
    }
    for container in [
        "prodml23.AssetProductionVolumes",
        "eml23.Aggregate",
        "witsml14.well",
        "nonsense",
    ] {
        let judged = rules.containment(container);
        assert_eq!(judged.kind, None, "{container}");
        assert!(judged.is_consistent(), "{container}");
    }
    // A source that is not an object URI is no guess either.
    let edge = judge_edges(&rules, vec![Edge::new("not a uri", GRID, "x")]).remove(0);
    assert_eq!(edge.kind, None);
}

/// KIND-4, the discovery half. The refusal is **met**, not global: a request by
/// kind answers while the walk touches only edges that have a kind, and refuses
/// by name, with the type and the cause, at the first edge that has none (ETP
/// v1.2 section 8.1.1.1.4 steps 1 and 2, p. 158; the refusal itself is this
/// crate's: no kind is guessed, section 8.1.1.1.2 note 1).
///
/// To break it: restore the global test in `discover` (refuse when any edge of
/// the dataspace has no kind); the first assertion fails. Or drop the check in
/// the walk; the second fails.
#[test]
fn kind_4_the_refusal_is_met_not_global() {
    let rules = rules_where_xsd_says(true);
    let resources: Vec<ObjectSummary> = [GRID, HORIZON, FLOW_TEST, CHANNEL_21].map(summary).to_vec();
    let edges = judge_edges(
        &rules,
        vec![
            Edge::new(GRID, HORIZON, "RepresentedInterpretation"),
            // The PRODML object is in the dataspace and has an edge without a kind.
            Edge::new(FLOW_TEST, CHANNEL_21, "ChannelSet"),
        ],
    );
    let mut context = Context::of(GRID, Scope::Targets);
    context.navigable = Navigable::Primary;

    // The walk from the RESQML object never touches the PRODML edge: it answers.
    let found = discover("eml:///", resources.clone(), edges.clone(), &context)
        .expect("a walk that meets only edges with a kind answers");
    let mut reached: Vec<&str> = found.resources.iter().map(|r| r.uri.as_str()).collect();
    reached.sort_unstable();
    assert_eq!(reached, vec![HORIZON]);

    // The walk that reaches the PRODML edge refuses, naming the type and the cause.
    let mut from_prodml = Context::of(FLOW_TEST, Scope::Targets);
    from_prodml.navigable = Navigable::Primary;
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
