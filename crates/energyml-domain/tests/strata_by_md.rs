//! A wellbore's stratigraphy carried by MD onto its other representations —
//! `energyml_domain::strata_by_md`.
//!
//! Every object is built in memory after the specification, not after a
//! package: RESQML TRG 2.2 §5.34 (p. 130) for `IntervalStratigraphicUnits`,
//! §12.13 (p. 310) for a frame's `NodeMd` — *"MD values and UOM must be
//! consistent with the trajectory representation"* —, §12.8 (p. 304) for
//! `MdDomain`, §12.14 (p. 311) for a wellbore interpretation *"relative to one
//! particular well trajectory"*; Energistics common TRG 2.3 §6.48 (p. 134) for
//! *"All coordinates with the same datum (and same UOM) can be considered to
//! be in the same coordinate reference system (CRS) and are thus directly
//! comparable."*
//!
//! # What it would take to make each one fail
//!
//! - moving a limit to the other side — the node at MD 10 below is on the
//!   limit between two units of different colours, and the one at MD 40 on the
//!   deepest limit;
//! - giving a node outside every interval its neighbour's unit — the nodes
//!   above 0 and below 40 must come back transparent;
//! - comparing MDs from another datum, another MD domain, or in feet as if in
//!   metres — each has its own test, and the feet one has nodes that land in
//!   different units once converted;
//! - picking one of two frames that disagree;
//! - joining representations by their feature rather than their
//!   interpretation, or not at all.
//!
//! Each was broken once on purpose while writing this file (see the report of
//! the change), and the test that names it went red.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::color::{self, GraphicalSet, Rgba, Scope};
use energyml_domain::source::{ObjectSource, Prefetched};
use energyml_domain::strata_by_md::{self, MdReference, Node, Painting, Stated};
use energyml_domain::well_strata::{self, Known, Refusal, Wanted};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::ArrayLocation;

const WELLBORE: &str = "e0000000-0000-4000-8000-000000000001";
const OTHER_WELLBORE: &str = "e0000000-0000-4000-8000-000000000002";
const DATUM: &str = "e0000000-0000-4000-8000-0000000000d1";
const OTHER_DATUM: &str = "e0000000-0000-4000-8000-0000000000d2";
const FEATURE: &str = "f0000000-0000-4000-8000-0000000000fe";
const RANK: &str = "a0000000-0000-4000-8000-000000000001";
const PRIABONIAN: &str = "b0000000-0000-4000-8000-000000000001";
const BARTONIAN: &str = "b0000000-0000-4000-8000-000000000002";
const LUTETIAN: &str = "b0000000-0000-4000-8000-000000000003";
const SET: &str = "d0000000-0000-4000-8000-000000000001";

const RED: Rgba = Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
const BLUE: Rgba = Rgba { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
const GREEN: Rgba = Rgba { r: 0.0, g: 1.0, b: 0.0, a: 1.0 };
const GONE: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

// -- building blocks ---------------------------------------------------------

fn nowhere(_: &dyn Reflect) -> Vec<ArrayLocation> {
    Vec::new()
}

fn held(objects: Vec<Box<dyn DynObject>>) -> Prefetched {
    let mut cache = Prefetched::new();
    for object in objects {
        cache.insert(object, &nowhere);
    }
    cache
}

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation { title: title.to_string(), ..Default::default() }
}

fn doubles(values: &[f64]) -> eml23::AbstractFloatingPointArray {
    eml23::AbstractFloatingPointArray::FloatingPointXmlArray(eml23::FloatingPointXmlArray {
        count_per_value: 1,
        values: values.to_vec(),
        ..Default::default()
    })
}

fn integers(values: &[i64]) -> eml23::IntegerXmlArray {
    eml23::IntegerXmlArray { count_per_value: 1, null_value: -1, values: values.to_vec(), ..Default::default() }
}

/// How a trajectory counts its MDs.
struct Counted {
    datum: &'static str,
    uom: eml23::LengthUomExt,
    domain: Option<resqml22::MdDomain>,
}

const METRES: Counted = Counted { datum: DATUM, uom: eml23::LengthUomExt::M, domain: Some(resqml22::MdDomain::Driller) };

/// A 2.2 trajectory of `wellbore`, with no geometry: MD 0 to 40.
fn trajectory(uuid: &str, wellbore: &str, counted: Counted) -> resqml22::WellboreTrajectoryRepresentation {
    resqml22::WellboreTrajectoryRepresentation {
        uuid: uuid.to_string(),
        citation: citation(&format!("{uuid} — trajectory")),
        represented_object: Some(dor(wellbore, "resqml22.WellboreInterpretation", "wellbore")),
        md_interval: eml23::MdInterval {
            md_min: 0.0,
            md_max: 40.0,
            uom: counted.uom,
            datum: Some(dor(counted.datum, "eml23.ReferencePointInACrs", "datum")),
            ..Default::default()
        },
        md_domain: counted.domain,
        ..Default::default()
    }
}

/// A 2.2 frame of `wellbore` on `trajectory`, at these MDs.
fn frame(
    uuid: &str,
    wellbore: &str,
    trajectory: &str,
    mds: &[f64],
    maps: Vec<resqml22::IntervalStratigraphicUnits>,
) -> resqml22::WellboreFrameRepresentation {
    resqml22::WellboreFrameRepresentation {
        uuid: uuid.to_string(),
        citation: citation(&format!("{uuid} — frame")),
        represented_object: Some(dor(wellbore, "resqml22.WellboreInterpretation", "wellbore")),
        node_count: mds.len() as i64,
        node_md: Some(doubles(mds)),
        trajectory: dor(trajectory, "resqml22.WellboreTrajectoryRepresentation", "trajectory"),
        interval_stratigraphic_units: maps,
        ..Default::default()
    }
}

/// One unit index per interval, through [`RANK`].
fn mapping(indices: &[i64]) -> resqml22::IntervalStratigraphicUnits {
    let ends: Vec<i64> = (1..=indices.len() as i64).collect();
    resqml22::IntervalStratigraphicUnits {
        unit_indices: eml23::JaggedArray {
            elements: Some(eml23::AbstractValueArray::IntegerXmlArray(integers(indices))),
            cumulative_length: Some(eml23::AbstractIntegerArray::IntegerXmlArray(integers(&ends))),
        },
        stratigraphic_organization_interpretation: dor(RANK, "resqml22.StratigraphicColumnRankInterpretation", "rank"),
    }
}

fn the_rank() -> Box<dyn DynObject> {
    Box::new(resqml22::StratigraphicColumnRankInterpretation {
        uuid: RANK.to_string(),
        citation: citation("stages"),
        ascending_ordering_criteria: resqml22::OrderingCriteria::Age,
        rank_in_stratigraphic_column: 1,
        stratigraphic_units: [PRIABONIAN, BARTONIAN, LUTETIAN]
            .iter()
            .map(|unit| dor(unit, "resqml22.StratigraphicUnitInterpretation", "unit"))
            .collect(),
        interpreted_feature: dor(FEATURE, "resqml22.RockVolumeFeature", "column"),
        ..Default::default()
    })
}

fn unit(uuid: &str, title: &str) -> Box<dyn DynObject> {
    Box::new(resqml22::StratigraphicUnitInterpretation {
        uuid: uuid.to_string(),
        citation: citation(title),
        interpreted_feature: dor(FEATURE, "resqml22.RockVolumeFeature", "rock"),
        ..Default::default()
    })
}

/// Priabonian red, Bartonian blue, Lutetian green.
fn graphics() -> Vec<(String, GraphicalSet)> {
    let mut set = eml23::GraphicalInformationSet::default();
    for (uuid, rgba) in [(PRIABONIAN, RED), (BARTONIAN, BLUE), (LUTETIAN, GREEN)] {
        color::set_constant_colour(
            set.as_reflect_mut(),
            &[color::Decorated {
                uuid: uuid.to_string(),
                qualified_type: "resqml22.StratigraphicUnitInterpretation".to_string(),
                title: "unit".to_string(),
            }],
            Scope::WholeObject,
            rgba,
            None,
        )
        .expect("the colour is written");
    }
    vec![(SET.to_string(), GraphicalSet::read(set.as_reflect()))]
}

/// The package every test starts from: the rank, its three units, and the
/// stating frame `S` on trajectory `T1` — Priabonian from MD 0 to 10,
/// Bartonian from 10 to 25, Lutetian from 25 to 40.
const T1: &str = "70000000-0000-4000-8000-000000000001";
const S: &str = "50000000-0000-4000-8000-000000000001";

fn base() -> Vec<Box<dyn DynObject>> {
    vec![
        the_rank(),
        unit(PRIABONIAN, "Priabonian"),
        unit(BARTONIAN, "Bartonian"),
        unit(LUTETIAN, "Lutetian"),
        Box::new(trajectory(T1, WELLBORE, METRES)),
        Box::new(frame(S, WELLBORE, T1, &[0.0, 10.0, 25.0, 40.0], vec![mapping(&[0, 1, 2])])),
    ]
}

/// Paint `target` with the frames of `objects`.
fn paint(objects: Vec<Box<dyn DynObject>>, target: &str) -> Result<Painting, Refusal> {
    paint_counted(objects, target).0
}

/// [`paint`], and how many times the frames were asked for.
fn paint_counted(objects: Vec<Box<dyn DynObject>>, target: &str) -> (Result<Painting, Refusal>, usize) {
    let source = held(objects);
    let sets = graphics();
    let asked = std::cell::Cell::new(0);
    // The package's frames, as a caller that does not index them gives them —
    // every one, of whichever wellbore: telling them apart is the library's job.
    let frames_of = |_wellbore: &str| -> Vec<Box<dyn DynObject>> {
        asked.set(asked.get() + 1);
        source
            .uuids()
            .iter()
            .filter_map(|uuid| source.object(uuid))
            .filter(|object| object.as_reflect().type_info().type_key == "WellboreFrameRepresentation")
            .collect()
    };
    let target = source.object(target).expect("the target is held");
    let painting = strata_by_md::of_representation(
        target.as_reflect(),
        &frames_of,
        None,
        &source,
        &Known { graphics: &sets, units: &[] },
    );
    (painting, asked.get())
}

fn hex(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(4)
        .map(|rgba| format!("#{:02x}{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2], rgba[3]))
        .collect()
}

fn colours(painting: &Painting) -> Vec<String> {
    hex(&painting.node_colours_with(GONE, &[]))
}

const R: &str = "#ff0000ff";
const B: &str = "#0000ffff";
const G: &str = "#00ff00ff";
const NONE: &str = "#00000000";

// -- the rule ------------------------------------------------------------------

/// A frame of the same wellbore that states no unit, on another trajectory with
/// the same datum, unit and domain: each node takes the interval it falls in.
/// On a limit, the interval **it opens** (MD 10 → Bartonian, MD 25 → Lutetian);
/// on the deepest limit, the last interval (MD 40 → Lutetian); outside, nothing.
#[test]
fn a_silent_frame_is_painted_by_its_wellbores_intervals_at_its_own_mds() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    const N: &str = "50000000-0000-4000-8000-000000000002";
    let mut objects = base();
    objects.push(Box::new(trajectory(T2, WELLBORE, METRES)));
    objects.push(Box::new(frame(N, WELLBORE, T2, &[-5.0, 0.0, 5.0, 10.0, 24.9, 25.0, 40.0, 50.0], vec![])));

    let Painting::Carried(carried) = paint(objects, N).expect("the silent frame is painted") else {
        panic!("a frame that states no unit is not its own painting");
    };
    assert_eq!(colours(&Painting::Carried(carried.clone())), [NONE, R, R, B, B, G, G, NONE]);
    assert_eq!(carried.outside(), 2, "above 0 and below 40, nothing: {:?}", carried.nodes);
    assert_eq!(carried.nodes[0], Node::Outside);
    assert_eq!(carried.from, [S.to_string()]);
    assert!(carried.notes.iter().any(|note| note.contains("2 of 8 node(s) lie outside")), "{:?}", carried.notes);
    // The legend's units are the stating frame's, in its rank's order.
    let titles: Vec<&str> = carried.strata.units.iter().map(|unit| unit.title.as_str()).collect();
    assert_eq!(titles, ["Priabonian", "Bartonian", "Lutetian"]);
}

/// A trajectory is drawn at its knots, and a trajectory's parameter **is** its
/// MD; one with no geometry at its `MdMin` and `MdMax`.
#[test]
fn a_trajectory_is_painted_at_its_knots_or_its_bounds() {
    const T3: &str = "70000000-0000-4000-8000-000000000003";
    let mut objects = base();
    let mut with_line = trajectory(T3, WELLBORE, METRES);
    with_line.geometry = Some(resqml22::AbstractParametricLineGeometry::ParametricLineGeometry(
        resqml22::ParametricLineGeometry {
            control_point_parameters: Some(doubles(&[0.0, 20.0, 30.0, 45.0])),
            knot_count: 4,
            ..Default::default()
        },
    ));
    objects.push(Box::new(with_line));

    let at_knots = paint(objects, T3).expect("the trajectory with a line is painted");
    assert_eq!(colours(&at_knots), [R, B, G, NONE]);

    // The stating frame's own trajectory, with no geometry: MD 0 and 40.
    let at_bounds = paint(base(), T1).expect("the trajectory with no geometry is painted");
    assert_eq!(colours(&at_bounds), [R, G]);
}

/// A line with no parameters has knots 0, 1, 2 … — no MD. Refused by name,
/// not painted at those numbers.
#[test]
fn a_line_without_parameters_has_no_md_to_look_up() {
    const T3: &str = "70000000-0000-4000-8000-000000000003";
    let mut objects = base();
    let mut with_line = trajectory(T3, WELLBORE, METRES);
    with_line.geometry = Some(resqml22::AbstractParametricLineGeometry::ParametricLineGeometry(
        resqml22::ParametricLineGeometry { knot_count: 3, ..Default::default() },
    ));
    objects.push(Box::new(with_line));
    let refused = paint(objects, T3).expect_err("no parameter is no MD");
    assert!(refused.to_string().contains("ControlPointParameters"), "{refused}");
}

/// A frame that states its own units is its own painting — the carried rule
/// never overrides what a frame says for itself.
#[test]
fn a_stating_frame_is_its_own_painting() {
    let painting = paint(base(), S).expect("the frame states its units");
    assert!(matches!(painting, Painting::Own(_)), "{painting:?}");
    assert_eq!(colours(&painting), [R, B, G, G]);
}

/// The wellbore's frames are asked for **only when they are needed** — never
/// for a frame that states its own units, once for one that does not. Building
/// the list first was 1,15 s a well on a package of 2 697 stating frames
/// (01/10), every second of it spent on a list nobody read.
///
/// To break it: ask for the list before knowing whether the frame states its units.
#[test]
fn the_wellbores_frames_are_asked_for_only_when_needed() {
    let (stating, asked) = paint_counted(base(), S);
    assert!(matches!(stating, Ok(Painting::Own(_))), "{stating:?}");
    assert_eq!(asked, 0, "a stating frame needs no other frame");

    const T2: &str = "70000000-0000-4000-8000-000000000002";
    const N: &str = "50000000-0000-4000-8000-000000000002";
    let mut objects = base();
    objects.push(Box::new(trajectory(T2, WELLBORE, METRES)));
    objects.push(Box::new(frame(N, WELLBORE, T2, &[0.0, 10.0], vec![])));
    let (silent, asked) = paint_counted(objects, N);
    assert!(matches!(silent, Ok(Painting::Carried(_))), "{silent:?}");
    assert_eq!(asked, 1, "a silent frame asks once, for its own wellbore");
}

/// The limit side is the one `Strata::node_colours` gives a frame's own nodes:
/// looking the stating frame's **own** MDs up gives back its own colours.
#[test]
fn the_limit_rule_is_the_frames_own_node_rule() {
    let source = held(base());
    let sets = graphics();
    let known = Known { graphics: &sets, units: &[] };
    let s = source.object(S).expect("held");
    let strata = well_strata::of_frame(s.as_reflect(), &Wanted::IntervalUnits { rank: None }, &source, &known)
        .expect("read");
    let reference = strata_by_md::md_reference(s.as_reflect(), &source).expect("its trajectory");
    let mds: Vec<f64> = vec![0.0, 10.0, 25.0, 40.0];
    let carried = strata_by_md::carry("x", "x", &mds, &reference, &[Stated { strata: &strata, reference: &reference }])
        .expect("carried");
    assert_eq!(hex(&carried.node_colours_with(GONE, &[])), hex(&strata.node_colours(GONE)));
}

// -- comparability -------------------------------------------------------------

/// Another datum: refused, naming both — never shifted.
#[test]
fn another_datum_is_refused_by_name() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    let mut objects = base();
    objects.push(Box::new(trajectory(
        T2,
        WELLBORE,
        Counted { datum: OTHER_DATUM, uom: eml23::LengthUomExt::M, domain: Some(resqml22::MdDomain::Driller) },
    )));
    let refused = paint(objects, T2).expect_err("two datums");
    assert!(matches!(refused, Refusal::Incomparable { .. }), "{refused:?}");
    let said = refused.to_string();
    assert!(said.contains(DATUM) && said.contains(OTHER_DATUM), "{said}");
}

/// The datum not stated is not the same datum.
#[test]
fn an_unstated_datum_is_refused() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    let mut objects = base();
    let mut unanchored = trajectory(T2, WELLBORE, METRES);
    unanchored.md_interval.datum = None;
    objects.push(Box::new(unanchored));
    let refused = paint(objects, T2).expect_err("no datum");
    assert!(matches!(refused, Refusal::Incomparable { .. }), "{refused:?}");
}

/// Logger depths against driller depths (TRG 2.2 §12.8): refused.
#[test]
fn another_md_domain_is_refused() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    let mut objects = base();
    objects.push(Box::new(trajectory(
        T2,
        WELLBORE,
        Counted { datum: DATUM, uom: eml23::LengthUomExt::M, domain: Some(resqml22::MdDomain::Logger) },
    )));
    let refused = paint(objects, T2).expect_err("two domains");
    assert!(refused.to_string().contains("logger"), "{refused}");
}

/// MDs in feet against intervals in metres: converted, which is arithmetic.
/// 0 ft is Priabonian, 50 ft (15.24 m) Bartonian, 100 ft (30.48 m) Lutetian —
/// read as metres, 50 and 100 would both be outside.
#[test]
fn feet_are_converted_not_read_as_metres() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    const N: &str = "50000000-0000-4000-8000-000000000002";
    let mut objects = base();
    objects.push(Box::new(trajectory(
        T2,
        WELLBORE,
        Counted { datum: DATUM, uom: eml23::LengthUomExt::Ft, domain: Some(resqml22::MdDomain::Driller) },
    )));
    objects.push(Box::new(frame(N, WELLBORE, T2, &[0.0, 50.0, 100.0], vec![])));
    let painting = paint(objects, N).expect("feet convert");
    assert_eq!(colours(&painting), [R, B, G]);
}

/// The same datum, unit and domain compare; a same-trajectory pair needs
/// nothing more.
#[test]
fn comparable_reads_the_three_things_and_the_trajectory() {
    let reference = |trajectory: &str, datum: Option<&str>, uom: Option<&str>, domain: Option<&str>| MdReference {
        trajectory: trajectory.to_string(),
        datum: datum.map(str::to_string),
        uom: uom.map(str::to_string),
        domain: domain.map(str::to_string),
    };
    let a = reference("t1", Some("d"), Some("m"), Some("driller"));
    assert_eq!(strata_by_md::comparable(&a, &reference("t2", Some("d"), Some("m"), Some("driller"))), Ok(1.0));
    assert_eq!(strata_by_md::comparable(&reference("t1", None, None, None), &reference("t1", None, None, None)), Ok(1.0));
    assert!(strata_by_md::comparable(&a, &reference("t2", Some("d"), Some("m"), None)).is_err(), "a domain stated once");
    assert!(strata_by_md::comparable(&a, &reference("t2", Some("d"), None, Some("driller"))).is_err(), "no unit");
    assert!(strata_by_md::comparable(&a, &reference("t2", Some("d"), Some("parsec?"), Some("driller"))).is_err());
}

// -- which frames speak ----------------------------------------------------------

/// Two frames of one wellbore naming two units at one MD: the node is left
/// uncoloured and counted, and the notes name both — neither is chosen. Where
/// they agree, and where one names nothing (a null index), it is painted.
#[test]
fn two_frames_that_disagree_are_said_and_not_arbitrated() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    const S2: &str = "50000000-0000-4000-8000-000000000003";
    let mut objects = base();
    objects.push(Box::new(trajectory(T2, WELLBORE, METRES)));
    // Priabonian 0–10 (agrees), Lutetian 10–25 (disagrees with Bartonian),
    // nothing said 25–40 (a null index: silence, not a contradiction).
    objects.push(Box::new(frame(S2, WELLBORE, T1, &[0.0, 10.0, 25.0, 40.0], vec![mapping(&[0, 2, -1])])));

    let Painting::Carried(carried) = paint(objects, T2).expect("painted where they agree") else {
        panic!("a trajectory is always carried");
    };
    // MD 0 and 40 — the trajectory's two points.
    assert_eq!(colours(&Painting::Carried(carried.clone())), [R, G]);
    assert_eq!(carried.disagreements(), 0);

    // At its knots now, one of them in the disputed interval.
    let mut objects = base();
    let mut with_line = trajectory(T2, WELLBORE, METRES);
    with_line.geometry = Some(resqml22::AbstractParametricLineGeometry::ParametricLineGeometry(
        resqml22::ParametricLineGeometry {
            control_point_parameters: Some(doubles(&[5.0, 15.0, 30.0])),
            knot_count: 3,
            ..Default::default()
        },
    ));
    objects.push(Box::new(with_line));
    objects.push(Box::new(frame(S2, WELLBORE, T1, &[0.0, 10.0, 25.0, 40.0], vec![mapping(&[0, 2, -1])])));
    let Painting::Carried(carried) = paint(objects, T2).expect("painted where they agree") else {
        panic!("a trajectory is always carried");
    };
    assert_eq!(carried.nodes[1], Node::Disagreement);
    assert_eq!(colours(&Painting::Carried(carried.clone())), [R, NONE, G]);
    assert_eq!(carried.disagreements(), 1);
    let note = carried.notes.iter().find(|note| note.contains("two different units")).expect("said");
    assert!(note.contains("Bartonian") && note.contains("Lutetian"), "{note}");
}

/// Another wellbore's frame says nothing about this one — even through the
/// same datum. Nothing states this wellbore's units: an absence, named.
#[test]
fn another_wellbores_frame_is_not_used() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    let mut objects = base();
    objects.push(Box::new(trajectory(T2, OTHER_WELLBORE, METRES)));
    let refused = paint(objects, T2).expect_err("nothing states the other wellbore");
    assert!(refused.is_absence(), "{refused:?}");
    assert!(refused.to_string().contains(OTHER_WELLBORE), "{refused}");
}

/// A frame of the wellbore whose units cannot be read is a defect, said — not
/// skipped into a picture painted by the others.
#[test]
fn a_defective_frame_of_the_wellbore_is_not_hidden() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    const S2: &str = "50000000-0000-4000-8000-000000000003";
    let mut objects = base();
    objects.push(Box::new(trajectory(T2, WELLBORE, METRES)));
    // Index 7 in a rank of three.
    objects.push(Box::new(frame(S2, WELLBORE, T1, &[0.0, 10.0, 25.0, 40.0], vec![mapping(&[0, 7, 2])])));
    let refused = paint(objects, T2).expect_err("the defect is said");
    assert!(matches!(refused, Refusal::Defect { .. }), "{refused:?}");
}

/// A frame of the wellbore that states no unit is not asked — here it has no
/// `NodeMd` at all, which `of_frame` would refuse as a shape it cannot read,
/// and that refusal must not stop the frame that does state units from
/// painting the trajectory.
#[test]
fn a_silent_frame_that_cannot_be_read_stops_nothing() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    const N: &str = "50000000-0000-4000-8000-000000000002";
    let mut objects = base();
    objects.push(Box::new(trajectory(T2, WELLBORE, METRES)));
    let mut unreadable = frame(N, WELLBORE, T2, &[], vec![]);
    unreadable.node_md = None;
    objects.push(Box::new(unreadable));
    let painting = paint(objects, T2).expect("the silent frame is not asked");
    assert_eq!(colours(&painting), [R, G]);
}

/// A surface is neither: `of_frame`'s own refusal, naming `NodeMd`.
#[test]
fn what_has_no_md_keeps_its_refusal() {
    let mut objects = base();
    objects.push(Box::new(resqml22::TriangulatedSetRepresentation {
        uuid: "90000000-0000-4000-8000-000000000001".to_string(),
        citation: citation("a horizon"),
        ..Default::default()
    }));
    let refused = paint(objects, "90000000-0000-4000-8000-000000000001").expect_err("a surface has no MD");
    assert!(refused.to_string().contains("NodeMd"), "{refused}");
}

// -- the intervals along the MD ---------------------------------------------------

/// The colour a looked-up MD takes in `intervals`, as `colours` spells it — the
/// rule of a node: the interval it opens, `[from, to)`; the deepest limit
/// belongs to the last interval of a frame's own sections (`closed_last`), and
/// a zero-length interval `[md, md]` holds its own MD. Nothing: [`NONE`].
fn looked_up(intervals: &[strata_by_md::Interval], md: f64, closed_last: bool) -> String {
    let last = intervals.len().saturating_sub(1);
    let found = intervals.iter().enumerate().find(|(at, one)| {
        (one.from <= md && md < one.to)
            || (one.from == one.to && one.from == md)
            || (closed_last && *at == last && md == one.to)
    });
    match found {
        // `GONE` is what the painting draws for a node without a colour.
        Some((_, one)) => one.colour.map_or(NONE.to_string(), |colour| hex(&colour.to_bytes())[0].clone()),
        None => NONE.to_string(),
    }
}

/// Triangulation: at the nodes' own MDs, the intervals give back the colours
/// `node_colours_with` paints the nodes with — for a frame that states its own
/// units (limits included: 10 and 25 open the interval below, 40 closes the
/// last), and for a silent frame carried by MD (nodes outside every interval
/// are a gap, 0 and 50 included).
///
/// To break it: make the limit belong to the interval it closes
/// (`from < md && md <= to`) — MD 10 and 25 change colour.
#[test]
fn at_the_nodes_own_mds_the_intervals_give_the_nodes_colours() {
    // A frame stating its own units.
    let own = paint(base(), S).expect("the frame states its units");
    let mds = [0.0, 10.0, 25.0, 40.0];
    let intervals = own.intervals(&mds, &[]).expect("intervals");
    assert_eq!(
        intervals.iter().map(|one| (one.from, one.to, one.unit.as_str())).collect::<Vec<_>>(),
        [(0.0, 10.0, "Priabonian"), (10.0, 25.0, "Bartonian"), (25.0, 40.0, "Lutetian")]
    );
    let from_intervals: Vec<String> = mds.iter().map(|md| looked_up(&intervals, *md, true)).collect();
    assert_eq!(from_intervals, colours(&own));
    assert_eq!(from_intervals, [R, B, G, G]);
    // Each interval names its unit by **uri** — what a reader finds the unit by,
    // instead of guessing from its title and its colour (05/10, I7).
    let units: Vec<Option<(String, bool)>> = intervals
        .iter()
        .map(|one| {
            one.unit_uri.as_deref().map(|uri| {
                (energyml_core::identity::parse_address(uri).0, uri.contains("StratigraphicUnitInterpretation"))
            })
        })
        .collect();
    assert_eq!(
        units,
        [PRIABONIAN, BARTONIAN, LUTETIAN].map(|uuid| Some((uuid.to_string(), true)))
    );

    // A silent frame, carried: nodes outside are a gap, a deeper node of the
    // same unit extends the interval it opened.
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    const N: &str = "50000000-0000-4000-8000-000000000002";
    let nodes = [-5.0, 0.0, 5.0, 10.0, 24.9, 25.0, 40.0, 50.0];
    let mut objects = base();
    objects.push(Box::new(trajectory(T2, WELLBORE, METRES)));
    objects.push(Box::new(frame(N, WELLBORE, T2, &nodes, vec![])));
    let carried = paint(objects, N).expect("carried");
    let intervals = carried.intervals(&nodes, &[]).expect("intervals");
    let from_intervals: Vec<String> = nodes.iter().map(|md| looked_up(&intervals, *md, false)).collect();
    assert_eq!(from_intervals, colours(&carried));
    assert_eq!(from_intervals, [NONE, R, R, B, B, G, G, NONE]);
    // The intervals are the ones the frame states, cut to the nodes' reach —
    // the Lutetian ends at 40, where the frame ends it, and node 40, on the
    // closed base of the last interval, holds its colour in a zero-length one.
    // Until 03/10 this said `(25, 50)`: the unit of node 40 stretched to node
    // 50, which the line above says is outside every interval.
    assert_eq!(
        intervals.iter().map(|one| (one.from, one.to, one.unit.as_str())).collect::<Vec<_>>(),
        [(0.0, 10.0, "Priabonian"), (10.0, 25.0, "Bartonian"), (25.0, 40.0, "Lutetian"), (40.0, 40.0, "Lutetian")]
    );

    // A trajectory with no geometry: its two bounds, the deepest node a
    // zero-length interval that still finds its colour.
    let bounds = paint(base(), T1).expect("the trajectory is painted");
    let intervals = bounds.intervals(&[0.0, 40.0], &[]).expect("intervals");
    assert_eq!(looked_up(&intervals, 0.0, false), R);
    assert_eq!(looked_up(&intervals, 40.0, false), G);
    assert_eq!(colours(&bounds), [R, G]);
}

/// A carried painting draws **the rock its frames state**, wherever its nodes
/// fall: two nodes far apart see every interval the frame states between them,
/// and nothing below the frame's last base — Lutetia's case (03/10), a frame
/// prolonged to 200 m painted from one that stops at its last described unit.
///
/// To break it: stretch a node's unit to the next node — the Lutetian runs to 70.
#[test]
fn a_carried_painting_draws_what_the_frames_state_and_nothing_below() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    const N: &str = "50000000-0000-4000-8000-000000000002";
    let nodes = [5.0, 30.0, 70.0];
    let mut objects = base();
    objects.push(Box::new(trajectory(T2, WELLBORE, METRES)));
    objects.push(Box::new(frame(N, WELLBORE, T2, &nodes, vec![])));
    let carried = paint(objects, N).expect("carried");
    let intervals = carried.intervals(&nodes, &[]).expect("intervals");
    assert_eq!(
        intervals.iter().map(|one| (one.from, one.to, one.unit.as_str())).collect::<Vec<_>>(),
        [(5.0, 10.0, "Priabonian"), (10.0, 25.0, "Bartonian"), (25.0, 40.0, "Lutetian")]
    );
    // And the nodes still find what `node_colours_with` gives them.
    let from_intervals: Vec<String> = nodes.iter().map(|md| looked_up(&intervals, *md, false)).collect();
    assert_eq!(from_intervals, colours(&carried));
    assert_eq!(from_intervals, [R, G, NONE]);
}

/// A unit the file does not colour has no colour in the interval (`None`: the
/// section view draws the well's own, never a transparent one) — or the
/// caller's palette, as `node_colours_with` does with the same `fallback`.
#[test]
fn a_unit_without_a_colour_is_none_or_the_callers_palette() {
    // The same package, but its colours are not read: no unit is coloured.
    let source = held(base());
    let target = source.object(S).expect("held");
    let painting = strata_by_md::of_representation(
        target.as_reflect(),
        &|_: &str| Vec::new(),
        None,
        &source,
        &Known::default(),
    )
    .expect("painted");
    let mds = [0.0, 10.0, 25.0, 40.0];
    let plain = painting.intervals(&mds, &[]).expect("intervals");
    assert!(plain.iter().all(|one| one.colour.is_none()), "{plain:?}");

    let palette = [RED, BLUE];
    let drawn = painting.intervals(&mds, &palette).expect("intervals");
    assert_eq!(
        drawn.iter().map(|one| one.colour.map(|c| c.to_hex())).collect::<Vec<_>>(),
        [Some(R.to_string()), Some(B.to_string()), Some(R.to_string())]
    );
    // And the nodes, with that palette, are painted the same.
    let from_intervals: Vec<String> = mds.iter().map(|md| looked_up(&drawn, *md, true)).collect();
    assert_eq!(from_intervals, hex(&painting.node_colours_with(GONE, &palette)));
}

/// An MD per node, or a refusal: a carried painting given the wrong number of
/// MDs would place rock where it is not.
#[test]
fn intervals_refuse_a_count_of_mds_that_is_not_the_nodes() {
    const T2: &str = "70000000-0000-4000-8000-000000000002";
    const N: &str = "50000000-0000-4000-8000-000000000002";
    let mut objects = base();
    objects.push(Box::new(trajectory(T2, WELLBORE, METRES)));
    objects.push(Box::new(frame(N, WELLBORE, T2, &[0.0, 10.0, 30.0], vec![])));
    let carried = paint(objects, N).expect("carried");
    let refused = carried.intervals(&[0.0, 10.0], &[]).expect_err("two MDs for three nodes");
    assert!(refused.contains("3 node(s)") && refused.contains("2 measured depth(s)"), "{refused}");
}
