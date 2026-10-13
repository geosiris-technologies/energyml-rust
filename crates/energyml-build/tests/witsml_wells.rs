//! `witsml-wells` — the WITSML 2.1 side of RESQML wells: what it writes, how
//! the objects name each other, and what it refuses.
//!
//! **The form of the assertions comes from the standards**, cited where used:
//!
//! - a `Wellbore` names its `Well` (1..1), and a sidetrack its
//!   `ParentWellbore` — *“No parent = starts from top”* (WITSML TRG 2.1 §20.2
//!   p. 502);
//! - a definitive trajectory *“must define the geometry of the whole wellbore
//!   (surface to bottom)”*, and one tied into another names it —
//!   `ParentTrajectory`, `MdTieOn` (§16.6 p. 350–351);
//! - a station's azimuth runs clockwise from north: *“0.00 and 360.00
//!   represent North, 90.00 is East, 180.00 is South and 270.00 is West”*
//!   (§16.10 p. 357, `ClosureDirection`, the same north as `Azi`), and its
//!   inclination is *“measured from vertical”* (p. 358);
//! - a vertical depth is *“positive moving downward from the reference
//!   datum”* (eml 2.3 `AbstractVerticalDepth`);
//! - on the RESQML side, a frame names its trajectory and the trajectory its
//!   datum (RESQML 2.2 `Wells.xsd`, `MdInterval.Datum`).
//!
//! **The numbers are the test's own**: the holes are drawn here, straight lines
//! and circles of radius `R = 30 / (15° in radians)` — an arc at 15°/30 m — at
//! their Lambert-93 places, because a defect found on 24/09 only showed far
//! from the origin. Every JSON the plan writes is read back into the generated
//! WITSML types before anything is asserted of it.

use energyml_build::operation::{Choices, Fitness, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_core::registry::DynObject;
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_witsml_2_1 as witsml;

const COMPOUND: &str = "0c0c0c0c-0000-4000-8000-000000000001";
const PROJECTED: &str = "0c0c0c0c-0000-4000-8000-000000000002";
const CAMPANIAN: &str = "0c0c0c0c-0000-4000-8000-000000000003";

fn when() -> When {
    When { creation: "2026-09-24T00:00:00Z".into(), originator: "test".into() }
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("witsml-wells", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn near(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

/// The radius of an arc at 15°/30 m: 114.59 m.
fn radius() -> f64 {
    30.0 / 15f64.to_radians()
}

fn compound() -> Ref {
    Ref::new(COMPOUND, "eml23.LocalEngineeringCompoundCrs", "EPSG:2154 + EPSG:5720")
}

/// A RESQML wellbore as the package holds it: a frame on a trajectory, from a
/// datum, representing an interpretation.
#[derive(Clone)]
struct Hole {
    name: &'static str,
    /// The uuids of its frame, trajectory and interpretation.
    frame: &'static str,
    trajectory: &'static str,
    interpretation: &'static str,
    /// Its datum's uuid, its title, and where it is.
    datum: (&'static str, &'static str),
    head: [f64; 3],
    /// Each node: its measured depth, and where it is in the compound —
    /// Lambert-93 easting, northing, and an elevation (the compound counts
    /// up).
    nodes: Vec<(f64, [f64; 3])>,
}

/// GTH-3: vertical from its datum at 48 m, a node every 10 m to MD 200.
fn gth3() -> Hole {
    let head = [651_800.0, 6_861_200.0, 48.0];
    Hole {
        name: "GTH-3",
        frame: "33333333-0000-4000-8000-000000000001",
        trajectory: "33333333-0000-4000-8000-000000000002",
        interpretation: "33333333-0000-4000-8000-000000000003",
        datum: ("33333333-0000-4000-8000-000000000004", "GTH-3 — datum"),
        head,
        nodes: (0..=20).map(|k| k as f64 * 10.0).map(|md| (md, [head[0], head[1], head[2] - md])).collect(),
    }
}

/// GTH-3 ST1: leaves GTH-3 at MD 40, builds at 15°/30 m to 45° towards the
/// north — 90 m of arc, to MD 130 — then holds to MD 170. Its depths are
/// GTH-3's, from GTH-3's datum. On the arc, `θ` of turn is `R (1 − cos θ)`
/// north and `R sin θ` down from the kick-off.
fn st1() -> Hole {
    let parent = gth3();
    let r = radius();
    let (s, c) = (45f64.to_radians().sin(), 45f64.to_radians().cos());
    let nodes = (4..=17)
        .map(|k| k as f64 * 10.0)
        .map(|md| {
            let (north, down) = if md <= 130.0 {
                let theta = (md - 40.0) / r;
                (r * (1.0 - theta.cos()), r * theta.sin())
            } else {
                let theta = 90.0 / r;
                (r * (1.0 - theta.cos()) + (md - 130.0) * s, r * theta.sin() + (md - 130.0) * c)
            };
            (md, [parent.head[0], parent.head[1] + north, parent.head[2] - 40.0 - down])
        })
        .collect();
    Hole {
        name: "GTH-3 ST1",
        frame: "31313131-0000-4000-8000-000000000001",
        trajectory: "31313131-0000-4000-8000-000000000002",
        interpretation: "31313131-0000-4000-8000-000000000003",
        datum: parent.datum,
        head: parent.head,
        nodes,
    }
}

/// GTH-2: vertical to MD 30 from its datum at 47 m, an arc at 15°/30 m to the
/// horizontal towards the **west** — 90° is 180 m of hole, to MD 210 — then a
/// drain to MD 300. The drain is `R` west and `30 + R` down from the datum at
/// MD 210.
fn gth2() -> Hole {
    let head = [651_595.0, 6_861_145.0, 47.0];
    let r = radius();
    let nodes = (0..=30)
        .map(|k| k as f64 * 10.0)
        .map(|md| {
            let (west, down) = if md <= 30.0 {
                (0.0, md)
            } else if md <= 210.0 {
                let theta = (md - 30.0) / r;
                (r * (1.0 - theta.cos()), 30.0 + r * theta.sin())
            } else {
                (r + md - 210.0, 30.0 + r)
            };
            (md, [head[0] - west, head[1], head[2] - down])
        })
        .collect();
    Hole {
        name: "GTH-2",
        frame: "22222222-0000-4000-8000-000000000001",
        trajectory: "22222222-0000-4000-8000-000000000002",
        interpretation: "22222222-0000-4000-8000-000000000003",
        datum: ("22222222-0000-4000-8000-000000000004", "GTH-2 — datum"),
        head,
        nodes,
    }
}

/// The two pieces a frame of the package makes: its nodes, and what the
/// boundary knows of it (`read::pieces_of_object`).
fn frame_of(hole: &Hole) -> (Source, Known) {
    let points: Vec<[f64; 3]> = hole.nodes.iter().map(|(_, at)| *at).collect();
    let count = points.len() as u32;
    let shape = Source::Shape {
        geometry: Geometry {
            kind: "polyline",
            patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Polylines(vec![count]), closed: Vec::new() }],
        },
        closed: None,
    };
    let known = Known {
        reference: Some(Ref::new(hole.frame, "resqml22.WellboreFrameRepresentation", format!("{} — frame", hole.name))),
        crs: Some(compound()),
        crs_epsg: Some(2154),
        vertical_epsg: Some(5720),
        interpretation: Some(Ref::new(hole.interpretation, "resqml22.WellboreInterpretation", format!("{} — interpretation", hole.name))),
        vertical_uom: Some("m".into()),
        vertical_up: Some(true),
        node_count: Some(hole.nodes.len()),
        // The datum placed through an identity compound: where it is.
        wellhead: Some(hole.head),
        node_mds: Some(hole.nodes.iter().map(|(md, _)| *md).collect()),
        trajectory: Some(Ref::new(hole.trajectory, "resqml22.WellboreTrajectoryRepresentation", format!("{} — trajectory", hole.name))),
        datum: Some(Ref::new(hole.datum.0, "eml23.ReferencePointInALocalEngineeringCompoundCrs", hole.datum.1)),
        ..Default::default()
    };
    (shape, known)
}

/// The bag the front end and the recipe build: `frames`, `frames 2`, … and each
/// one's companion.
fn bag(frames: &[(Source, Known)]) -> Pieces {
    let mut pieces = Pieces::empty();
    for (at, (shape, known)) in frames.iter().enumerate() {
        let name = if at == 0 { "frames".to_string() } else { format!("frames {}", at + 1) };
        pieces = pieces.with(name.clone(), shape.clone()).with(format!("{name} (object)"), Source::Object(known.clone()));
    }
    pieces
}

fn holes(these: &[Hole]) -> Pieces {
    bag(&these.iter().map(frame_of).collect::<Vec<_>>())
}

/// The doublet's wells of the recipe, GTH-3 and its lateral and GTH-2.
fn doublet() -> Pieces {
    holes(&[gth3(), st1(), gth2()])
}

fn of_type<'a>(plan: &'a Plan, qualified: &str) -> Vec<&'a NewObject> {
    plan.objects.iter().filter(|object| object.qualified_type == qualified).collect()
}

fn titled<'a>(plan: &'a Plan, qualified: &str, title: &str) -> &'a NewObject {
    plan.objects
        .iter()
        .find(|object| object.qualified_type == qualified && object.title == title)
        .unwrap_or_else(|| panic!("a `{qualified}` titled `{title}` in the plan: {:?}", plan.objects.iter().map(|o| &o.title).collect::<Vec<_>>()))
}

/// The JSON of a plan's object, read back into its generated type — the road
/// `apply` takes. A field misspelt, a value of the wrong kind, and this panics.
fn read<T: energyml_core::value::Reflect + Clone>(object: &NewObject) -> T {
    let schemas = energyml_schemas::all();
    let document = energyml_core::json::read_value(&object.json, &schemas)
        .unwrap_or_else(|why| panic!("`{}` does not read back: {why}", object.title));
    let root: &dyn DynObject = &*document.root;
    root.as_reflect()
        .downcast_ref::<T>()
        .unwrap_or_else(|| panic!("`{}` does not read back as {}", object.title, std::any::type_name::<T>()))
        .clone()
}

fn trajectory(plan: &Plan, title: &str) -> witsml::Trajectory {
    read(titled(plan, "witsml21.Trajectory", title))
}

fn wellbore(plan: &Plan, title: &str) -> witsml::Wellbore {
    read(titled(plan, "witsml21.Wellbore", title))
}

fn mds(trajectory: &witsml::Trajectory) -> Vec<f64> {
    trajectory.trajectory_station.iter().map(|station| station.md.measured_depth.value).collect()
}

fn recorded(plan: &Plan, name: &str) -> Option<f64> {
    plan.activity.parameters.iter().find_map(|parameter| match parameter {
        Parameter::Double { title, value, .. } if title == name => Some(*value),
        _ => None,
    })
}

fn said<'a>(plan: &'a Plan, key: &str) -> Vec<&'a str> {
    plan.says.iter().filter(|note| note.key == key).map(|note| note.detail.as_str()).collect()
}

fn tvd_of(station: &witsml::TrajectoryStation) -> Option<f64> {
    match &station.tvd {
        Some(eml::AbstractVerticalDepth::DatumVerticalDepth(depth)) => Some(depth.vertical_depth.value),
        _ => None,
    }
}

// ---------------------------------------------------------------------------

#[test]
fn a_well_per_head_a_wellbore_per_resqml_wellbore_a_trajectory_per_resqml_trajectory() {
    // To break: one Well per frame (three here), wellbores keyed on the frame
    // rather than on the interpretation, a sidetrack filed under a Well of its
    // own.
    let plan = planned(&doublet(), Choices::new()).unwrap();
    assert_eq!(of_type(&plan, "witsml21.Well").len(), 2, "GTH-3 and its lateral share a head, GTH-2 has its own");
    assert_eq!(of_type(&plan, "witsml21.Wellbore").len(), 3);
    assert_eq!(of_type(&plan, "witsml21.Trajectory").len(), 3);

    let wells: Vec<witsml::Well> = of_type(&plan, "witsml21.Well").into_iter().map(read::<witsml::Well>).collect();
    let mut titles: Vec<&str> = wells.iter().map(|well| well.citation.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(titles, ["GTH-2", "GTH-3"], "a Well is named after the wellbore that starts from its top");

    let (gth3, lateral, gth2) = (wellbore(&plan, "GTH-3"), wellbore(&plan, "GTH-3 ST1"), wellbore(&plan, "GTH-2"));
    assert_eq!(lateral.well.uuid, gth3.well.uuid, "a sidetrack is a wellbore of its parent's well");
    assert_ne!(gth2.well.uuid, gth3.well.uuid);
    let gth3_well = titled(&plan, "witsml21.Well", "GTH-3");
    assert_eq!(gth3.well.uuid, gth3_well.uuid, "the Well a Wellbore names is the one written");
    assert_eq!(gth3.well.qualified_type, "witsml21.Well");

    // Each trajectory names its wellbore.
    for (title, bore) in [("GTH-3 — trajectory", "GTH-3"), ("GTH-3 ST1 — trajectory", "GTH-3 ST1"), ("GTH-2 — trajectory", "GTH-2")] {
        assert_eq!(trajectory(&plan, title).wellbore.uuid, titled(&plan, "witsml21.Wellbore", bore).uuid, "{title}");
    }
    // Parents before what names them: a Well, then its wellbores, the parent
    // first; each trajectory after its wellbore.
    let at = |title: &str| plan.objects.iter().position(|o| o.title == title).unwrap();
    assert!(at("GTH-3") < at("GTH-3 — trajectory") && at("GTH-3 — trajectory") < at("GTH-3 ST1"), "{:?}", plan.objects.iter().map(|o| &o.title).collect::<Vec<_>>());

    assert_eq!(recorded(&plan, "wellCount"), Some(2.0));
    assert_eq!(recorded(&plan, "wellboreCount"), Some(3.0));
    assert_eq!(recorded(&plan, "trajectoryCount"), Some(3.0));
    assert_eq!(recorded(&plan, "sidetrackCount"), Some(1.0));
    // 21 + (4 + 14) + 31.
    assert_eq!(recorded(&plan, "stationCount"), Some(70.0));
}

#[test]
fn a_sidetrack_names_its_parent_and_ties_on_at_its_kickoff() {
    // To break: the parent read from the order of the frames rather than from
    // where the lateral starts, the tie-in missing, the lateral's trajectory
    // starting at its kick-off and still called definitive.
    let plan = planned(&holes(&[st1(), gth3()]), Choices::new()).unwrap();
    let parent = titled(&plan, "witsml21.Wellbore", "GTH-3");
    let lateral = wellbore(&plan, "GTH-3 ST1");
    assert_eq!(lateral.parent_wellbore.as_ref().map(|p| p.uuid.as_str()), Some(parent.uuid.as_str()));
    assert_eq!(lateral.parent_wellbore.as_ref().map(|p| p.qualified_type.as_str()), Some("witsml21.Wellbore"));
    assert_eq!(lateral.type_wellbore, Some(witsml::WellboreType::Sidetrack));
    assert_eq!(lateral.md_kickoff.as_ref().map(|md| md.measured_depth.value), Some(40.0));
    // Vertical above the kick-off: 40 m below the datum.
    match &lateral.tvd_kickoff {
        Some(eml::AbstractVerticalDepth::DatumVerticalDepth(depth)) => assert!(near(depth.vertical_depth.value, 40.0, 1e-9)),
        other => panic!("a kick-off vertical depth: {other:?}"),
    }
    let root = wellbore(&plan, "GTH-3");
    assert_eq!(root.parent_wellbore, None, "“No parent = starts from top”");
    assert_eq!(root.type_wellbore, None, "what the first hole of a well was is not a fact of the package");

    let laterals = trajectory(&plan, "GTH-3 ST1 — trajectory");
    let parents = titled(&plan, "witsml21.Trajectory", "GTH-3 — trajectory");
    assert_eq!(laterals.parent_trajectory.as_ref().map(|p| p.uuid.as_str()), Some(parents.uuid.as_str()));
    assert_eq!(laterals.md_tie_on.as_ref().map(|md| md.measured_depth.value), Some(40.0));
    let ties: Vec<f64> = laterals
        .trajectory_station
        .iter()
        .filter(|station| station.type_traj_station == witsml::TrajStationType::TieInPoint)
        .map(|station| station.md.measured_depth.value)
        .collect();
    assert_eq!(ties, [40.0], "one tie-in, at the kick-off");
    // Surface to bottom: through the parent's stations above the kick-off.
    assert_eq!(laterals.definitive, Some(true));
    assert_eq!(mds(&laterals)[0], 0.0);
    assert_eq!(trajectory(&plan, "GTH-3 — trajectory").definitive, Some(true));
    assert_eq!(said(&plan, "sidetrack").len(), 1);
}

#[test]
fn the_stations_are_the_resqml_nodes_at_their_own_depths_and_places() {
    // “a given MD is never recomputed” — each station's MD is the frame's
    // own, bit for bit, and its location is the node, in the same compound.
    let plan = planned(&doublet(), Choices::new()).unwrap();
    let drain = trajectory(&plan, "GTH-2 — trajectory");
    assert_eq!(mds(&drain), gth2().nodes.iter().map(|(md, _)| *md).collect::<Vec<_>>());
    for (station, (md, at)) in drain.trajectory_station.iter().zip(&gth2().nodes) {
        match station.location.as_slice() {
            [eml::AbstractPosition::EngineeringCompoundPosition(position)] => {
                assert_eq!([position.coordinate1, position.coordinate2, position.vertical_coordinate], *at, "MD {md}");
                assert_eq!(position.local_engineering_compound_crs.uuid, COMPOUND);
            }
            other => panic!("MD {md}: one position in the compound, got {other:?}"),
        }
    }
    // The lateral: GTH-3's stations above the kick-off, then its own.
    let mut expected: Vec<f64> = gth3().nodes.iter().map(|(md, _)| *md).filter(|md| *md < 40.0).collect();
    expected.extend(st1().nodes.iter().map(|(md, _)| *md));
    assert_eq!(mds(&trajectory(&plan, "GTH-3 ST1 — trajectory")), expected);
    // The index and the interval say the same depths, from the same datum.
    assert_eq!(drain.index.index_kind, eml::DataIndexKind::MeasuredDepth);
    assert_eq!(drain.index.direction, eml::IndexDirection::Increasing);
    let interval = drain.md_interval.as_ref().expect("an interval, since there are stations");
    assert_eq!((interval.md_min, interval.md_max), (0.0, 300.0));
    assert_eq!(interval.datum.as_ref().map(|d| d.uuid.as_str()), Some(gth2().datum.0));
    assert_eq!(drain.default_md_datum.as_ref().map(|d| d.uuid.as_str()), Some(gth2().datum.0));
}

#[test]
fn a_vertical_hole_is_inclined_zero_and_has_no_azimuth() {
    let plan = planned(&doublet(), Choices::new()).unwrap();
    let vertical = trajectory(&plan, "GTH-3 — trajectory");
    for station in &vertical.trajectory_station {
        let md = station.md.measured_depth.value;
        assert_eq!(station.incl.as_ref().map(|a| a.value), Some(0.0), "MD {md}");
        assert_eq!(station.azi, None, "MD {md}: a vertical hole has no azimuth, and a zero would say north");
    }
    assert!(!said(&plan, "vertical-no-azimuth").is_empty());
}

#[test]
fn a_drain_is_horizontal_and_azimuths_run_clockwise_from_grid_north() {
    // West is 270 clockwise from north; counter-clockwise it would be 90. The
    // lateral holds towards the north: 0.
    let plan = planned(&doublet(), Choices::new()).unwrap();
    let drain = trajectory(&plan, "GTH-2 — trajectory");
    assert_eq!(drain.azi_ref, Some(eml::NorthReferenceKind::GridNorth), "the datum proves the compound's axes are Lambert-93's");
    let horizontal: Vec<&witsml::TrajectoryStation> =
        drain.trajectory_station.iter().filter(|s| s.md.measured_depth.value >= 210.0).collect();
    assert_eq!(horizontal.len(), 10);
    for station in horizontal {
        let md = station.md.measured_depth.value;
        let incl = station.incl.as_ref().expect("an inclination");
        let azi = station.azi.as_ref().expect("a horizontal hole has an azimuth");
        assert!(near(incl.value, 90.0, 1e-6), "MD {md}: {}", incl.value);
        assert!(near(azi.value, 270.0, 1e-6), "MD {md}: {}", azi.value);
        assert_eq!((incl.uom.clone(), azi.uom.clone()), (eml::PlaneAngleUom::Dega, eml::PlaneAngleUom::Dega));
    }
    // Half-way round the arc, MD 120: 90 m of hole at 0.5°/m is 45°.
    let half = drain.trajectory_station.iter().find(|s| s.md.measured_depth.value == 120.0).unwrap();
    assert!(near(half.incl.as_ref().unwrap().value, 45.0, 1e-6), "{:?}", half.incl);
    // The dogleg severity of the arc is its rate, per 30 m.
    let dls = half.dls.as_ref().expect("a dogleg severity");
    assert!(near(dls.value, 15.0, 1e-6), "{}", dls.value);
    assert_eq!(dls.uom, eml::AnglePerLengthUom::V1Per30DegaPerM);

    let lateral = trajectory(&plan, "GTH-3 ST1 — trajectory");
    let last = lateral.trajectory_station.last().unwrap();
    assert!(near(last.incl.as_ref().unwrap().value, 45.0, 1e-6), "{:?}", last.incl);
    let north = last.azi.as_ref().expect("an azimuth").value;
    assert!(north.min(360.0 - north) <= 1e-6, "towards the north: {north}");
}

#[test]
fn vertical_depths_and_displacements_are_from_the_datum() {
    // The drain at MD 300: 30 m of hold, then R of arc down, below the datum;
    // R of arc then 90 m of drain towards the west.
    let plan = planned(&doublet(), Choices::new()).unwrap();
    let drain = trajectory(&plan, "GTH-2 — trajectory");
    let last = drain.trajectory_station.last().unwrap();
    let r = radius();
    assert!(near(tvd_of(last).expect("a vertical depth"), 30.0 + r, 1e-9), "{:?}", last.tvd);
    assert!(near(last.disp_ew.as_ref().unwrap().value, -(r + 90.0), 1e-9), "{:?}", last.disp_ew);
    assert_eq!(last.disp_ns.as_ref().unwrap().value, 0.0);
    let first = &drain.trajectory_station[0];
    assert_eq!(tvd_of(first), Some(0.0), "the first station is the datum");

    let bore = wellbore(&plan, "GTH-2");
    assert_eq!(bore.default_md_datum.as_ref().map(|d| d.uuid.as_str()), Some(gth2().datum.0));
    assert_eq!(bore.default_tvd_datum.as_ref().map(|d| d.uuid.as_str()), Some(gth2().datum.0));
}

#[test]
fn the_same_frames_name_the_same_objects_and_a_moved_node_another_trajectory() {
    let first = planned(&doublet(), Choices::new()).unwrap();
    let again = planned(&doublet(), Choices::new()).unwrap();
    let uuids = |plan: &Plan| plan.objects.iter().map(|o| o.uuid.clone()).collect::<Vec<_>>();
    assert_eq!(uuids(&first), uuids(&again));

    // One more metre of drain: the same well, the same wellbore, another
    // trajectory — never the first one rewritten.
    let mut longer = gth2();
    let last = longer.nodes.last_mut().unwrap();
    last.0 += 1.0;
    last.1[0] -= 1.0;
    let moved = planned(&holes(&[gth3(), st1(), longer]), Choices::new()).unwrap();
    let uuid = |plan: &Plan, qualified: &str, title: &str| titled(plan, qualified, title).uuid.clone();
    assert_eq!(uuid(&moved, "witsml21.Well", "GTH-2"), uuid(&first, "witsml21.Well", "GTH-2"));
    assert_eq!(uuid(&moved, "witsml21.Wellbore", "GTH-2"), uuid(&first, "witsml21.Wellbore", "GTH-2"));
    assert_ne!(uuid(&moved, "witsml21.Trajectory", "GTH-2 — trajectory"), uuid(&first, "witsml21.Trajectory", "GTH-2 — trajectory"));
    assert_eq!(
        uuid(&moved, "witsml21.Trajectory", "GTH-3 — trajectory"),
        uuid(&first, "witsml21.Trajectory", "GTH-3 — trajectory"),
        "what did not move keeps its name"
    );
}

#[test]
fn every_object_reads_back_into_its_generated_type() {
    let plan = planned(&doublet(), Choices::new().set("existence", "planned")).unwrap();
    for object in &plan.objects {
        match object.qualified_type.as_str() {
            "witsml21.Well" => {
                let well: witsml::Well = read(object);
                assert_eq!(well.uuid, object.uuid);
                assert_eq!(well.schema_version, "2.1");
            }
            "witsml21.Wellbore" => {
                let bore: witsml::Wellbore = read(object);
                assert_eq!(bore.uuid, object.uuid);
            }
            "witsml21.Trajectory" => {
                let trajectory: witsml::Trajectory = read(object);
                assert_eq!(trajectory.uuid, object.uuid);
                assert!(!trajectory.trajectory_station.is_empty());
            }
            other => panic!("`witsml-wells` writes WITSML wells and nothing else, not {other}"),
        }
    }
    assert_eq!(plan.objects.len(), 8);
}

#[test]
fn what_the_wells_are_is_written_where_it_is_said() {
    // A planned hole has a planned depth, not a drilled one; an invented one
    // says so on every object.
    let plan = planned(&holes(&[gth2()]), Choices::new().set("existence", "planned").set("synthetic", true)).unwrap();
    let well: witsml::Well = read(titled(&plan, "witsml21.Well", "GTH-2"));
    assert_eq!(well.existence, Some(eml::ExistenceKindExt::Planned));
    assert_eq!(well.extension_name_value.len(), 1);
    assert_eq!((well.extension_name_value[0].name.as_str(), well.extension_name_value[0].value.value.as_str()), ("synthetic", "true"));
    assert!(well.citation.description.as_deref().is_some_and(|d| d.contains("invented")), "{:?}", well.citation.description);
    let bore = wellbore(&plan, "GTH-2");
    assert_eq!(bore.md_planned.as_ref().map(|md| md.measured_depth.value), Some(300.0));
    assert_eq!(bore.md, None, "a planned hole reached no depth");
    match &bore.tvd_planned {
        Some(eml::AbstractVerticalDepth::DatumVerticalDepth(depth)) => assert!(near(depth.vertical_depth.value, 30.0 + radius(), 1e-9)),
        other => panic!("a planned vertical depth: {other:?}"),
    }
    assert_eq!(trajectory(&plan, "GTH-2 — trajectory").extension_name_value.len(), 1);

    let drilled = planned(&holes(&[gth2()]), Choices::new().set("existence", "actual")).unwrap();
    let bore = wellbore(&drilled, "GTH-2");
    assert_eq!(bore.md.as_ref().map(|md| md.measured_depth.value), Some(300.0));
    assert_eq!(bore.md_planned, None);
    let well: witsml::Well = read(titled(&drilled, "witsml21.Well", "GTH-2"));
    assert!(well.extension_name_value.is_empty(), "nothing is synthetic unless it is said");

    // Said by nobody: neither, and the plan says so.
    let unsaid = planned(&holes(&[gth2()]), Choices::new()).unwrap();
    let bore = wellbore(&unsaid, "GTH-2");
    assert_eq!((bore.md.is_some(), bore.md_planned.is_some(), bore.existence.is_some()), (false, false, false));
    assert_eq!(said(&unsaid, "existence-unsaid").len(), 1);
}

#[test]
fn the_projected_system_locates_each_well_and_a_system_of_another_code_is_refused() {
    let projected = |code: i64| {
        Source::Object(Known {
            reference: Some(Ref::new(PROJECTED, "eml23.ProjectedCrs", "EPSG:2154")),
            crs: Some(Ref::new(PROJECTED, "eml23.ProjectedCrs", "EPSG:2154")),
            crs_epsg: Some(code),
            ..Default::default()
        })
    };
    let plan = planned(&holes(&[gth2()]).with("projected", projected(2154)), Choices::new()).unwrap();
    let well: witsml::Well = read(titled(&plan, "witsml21.Well", "GTH-2"));
    let at = well.informational_projected_location.as_ref().expect("a projected location");
    assert_eq!((at.coordinate1, at.coordinate2), (651_595.0, 6_861_145.0));
    assert_eq!(at.projected_crs.uuid, PROJECTED);
    // The surface location: the datum in the compound, and in the projected
    // system.
    assert_eq!(well.well_surface_location.len(), 2, "{:?}", well.well_surface_location);

    let wrong = planned(&holes(&[gth2()]).with("projected", projected(27572)), Choices::new()).unwrap_err();
    assert!(wrong.contains("`projected`") && wrong.contains("EPSG:27572") && wrong.contains("EPSG:2154"), "{wrong}");

    let without = planned(&holes(&[gth2()]), Choices::new()).unwrap();
    let well: witsml::Well = read(titled(&without, "witsml21.Well", "GTH-2"));
    assert_eq!(well.informational_projected_location, None);
    assert_eq!(well.well_surface_location.len(), 1, "the datum, in the compound");
}

#[test]
fn a_target_unit_is_every_wellbores_and_nothing_else_is_one() {
    let unit = |qualified: &str| {
        Source::Object(Known { reference: Some(Ref::new(CAMPANIAN, qualified, "Campanien")), ..Default::default() })
    };
    let plan = planned(&holes(&[gth2()]).with("target", unit("resqml22.StratigraphicUnitInterpretation")), Choices::new()).unwrap();
    let bore = wellbore(&plan, "GTH-2");
    assert_eq!(bore.target_geologic_unit_interpretation.as_ref().map(|t| t.uuid.as_str()), Some(CAMPANIAN));
    let wrong = planned(&holes(&[gth2()]).with("target", unit("resqml22.HorizonInterpretation")), Choices::new()).unwrap_err();
    assert!(wrong.contains("`target`"), "{wrong}");
}

#[test]
fn north_is_grid_north_only_where_the_datum_proves_it() {
    // The same GTH-2, its datum placed 100 m east of where the frame puts its
    // MD 0 station — a compound offset from Lambert-93. Its azimuths are the
    // frame's own, and `AziRef` says so.
    let (shape, mut known) = frame_of(&gth2());
    known.wellhead = Some([651_695.0, 6_861_145.0, 47.0]);
    let plan = planned(&bag(&[(shape, known)]), Choices::new()).unwrap();
    let drain = trajectory(&plan, "GTH-2 — trajectory");
    assert_eq!(drain.azi_ref, Some(eml::NorthReferenceKind::PlantNorth));
    assert_eq!(said(&plan, "plant-north").len(), 1);
}

#[test]
fn a_kinked_line_keeps_its_stations_and_is_given_no_angle() {
    // Vertical for 20 m, then straight on at 30° — the depths are the lengths
    // of the segments, as a hand-drawn line has them. No arc turns 30° in no
    // hole: the stations are written with their depths and places, without
    // an inclination nobody can state.
    let head = [651_000.0, 6_861_000.0, 35.0];
    let (s, c) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
    let mut nodes: Vec<(f64, [f64; 3])> = vec![(0.0, head), (10.0, [head[0], head[1], 25.0]), (20.0, [head[0], head[1], 15.0])];
    nodes.extend((1..=3).map(|k| {
        let d = k as f64 * 10.0;
        (20.0 + d, [head[0] + d * s, head[1], 15.0 - d * c])
    }));
    let kinked = Hole {
        name: "KINK",
        frame: "44444444-0000-4000-8000-000000000001",
        trajectory: "44444444-0000-4000-8000-000000000002",
        interpretation: "44444444-0000-4000-8000-000000000003",
        datum: ("44444444-0000-4000-8000-000000000004", "KINK — datum"),
        head,
        nodes,
    };
    let plan = planned(&holes(&[kinked]), Choices::new()).unwrap();
    let line = trajectory(&plan, "KINK — trajectory");
    assert_eq!(mds(&line), [0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);
    assert!(line.trajectory_station.iter().all(|s| s.incl.is_none() && s.azi.is_none() && s.dls.is_none()));
    assert_eq!(line.nominal_calc_algorithm, None);
    // 20 m of vertical hole, then 30 m at 30°.
    assert!(near(tvd_of(&line.trajectory_station[5]).expect("a vertical depth"), 20.0 + 30.0 * c, 1e-9));
    let why = said(&plan, "angles-not-derived");
    assert_eq!(why.len(), 1);
    assert!(why[0].contains("MD 20") && why[0].contains("MD 30"), "the corner is named by its depths: {}", why[0]);
}

#[test]
fn a_frame_without_a_datum_is_refused_by_name() {
    let (shape, mut known) = frame_of(&gth2());
    known.datum = None;
    let why = planned(&bag(&[(shape, known)]), Choices::new()).unwrap_err();
    assert!(why.contains("GTH-2 — frame") && why.contains("datum"), "{why}");
}

#[test]
fn a_frame_in_degrees_is_refused_by_name() {
    let (shape, mut known) = frame_of(&gth2());
    known.crs = Some(Ref::new("0c0c0c0c-0000-4000-8000-00000000000d", "eml23.Geographic2dCrs", "WGS 84"));
    let why = planned(&bag(&[(shape, known)]), Choices::new()).unwrap_err();
    assert!(why.contains("GTH-2 — frame") && why.contains("degrees"), "{why}");
}

#[test]
fn a_frame_that_does_not_say_which_way_is_down_is_refused() {
    let (shape, mut known) = frame_of(&gth2());
    known.vertical_up = None;
    let why = planned(&bag(&[(shape, known)]), Choices::new()).unwrap_err();
    assert!(why.contains("GTH-2 — frame") && why.contains("which way"), "{why}");
}

#[test]
fn a_frame_whose_depths_come_back_is_refused() {
    let (shape, mut known) = frame_of(&gth2());
    let mut depths = known.node_mds.clone().unwrap();
    depths[5] = depths[4];
    known.node_mds = Some(depths);
    let why = planned(&bag(&[(shape, known)]), Choices::new()).unwrap_err();
    assert!(why.contains("do not increase"), "{why}");
}

#[test]
fn two_wellbores_from_one_datum_that_do_not_meet_are_refused() {
    // A second hole measured from GTH-3's datum, starting at MD 50 a hundred
    // metres east of GTH-3: it leaves nothing, and it does not start from the
    // top. Which well it is, nothing here says.
    let stray = Hole {
        name: "GTH-3 bis",
        frame: "35353535-0000-4000-8000-000000000001",
        trajectory: "35353535-0000-4000-8000-000000000002",
        interpretation: "35353535-0000-4000-8000-000000000003",
        datum: gth3().datum,
        head: gth3().head,
        nodes: (5..=10).map(|k| k as f64 * 10.0).map(|md| (md, [651_900.0, 6_861_200.0, 48.0 - md])).collect(),
    };
    let why = planned(&holes(&[gth3(), stray]), Choices::new()).unwrap_err();
    assert!(why.contains("`GTH-3`") && why.contains("`GTH-3 bis`") && why.contains("No parent = starts from top"), "{why}");
}

#[test]
fn a_shape_with_nothing_known_of_it_is_fit_and_refused_when_planned() {
    // The offer judges the kind of piece; what a frame must say is the plan's
    // to refuse, by name (`crates/energyml-build/ARCHITECTURE.md`).
    let (shape, _) = frame_of(&gth2());
    let bare = Pieces::empty().with("frames", shape);
    let registry = Registry::new();
    assert_eq!(registry.get("witsml-wells").expect("in the registry").fitness(&bare), Fitness::Fit);
    let why = planned(&bare, Choices::new()).unwrap_err();
    assert!(why.contains("nothing known"), "{why}");
}

#[test]
fn what_is_not_written_is_said() {
    // The RESQML side of the link and what §6.7 asks beyond wells and
    // trajectories: left out, and said — never a silence that reads as done.
    let plan = planned(&doublet(), Choices::new()).unwrap();
    assert!(said(&plan, "resqml-links").iter().any(|d| d.contains("WitsmlWellbore") && d.contains("WitsmlTrajectory")));
    assert!(said(&plan, "left-out").iter().any(|d| d.contains("markers")));
    assert!(said(&plan, "angles").iter().any(|d| d.contains("derived")));
}
