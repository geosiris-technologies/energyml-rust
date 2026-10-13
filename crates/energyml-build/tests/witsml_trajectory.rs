//! **A WITSML trajectory read back where its RESQML well is** - the round trip
//! `witsml-wells` → `energyml_geometry` over two independent code paths.
//!
//! The writer derives each station's inclination and azimuth from positions:
//! `energyml_analysis::survey`, the minimum curvature method **inverted**. The
//! reader places the stations from those angles: `energyml_geometry::min_curvature`,
//! the method run **forward**, from the datum the trajectory names and in the
//! north its `AziRef` states (`energyml_geometry::stations`). Neither is asked
//! to check itself: what they must both agree with is the third source, the
//! holes drawn **by hand** here - straight lines and circles of radius
//! `R = 30 / (15° in radians)`, an arc at 15°/30 m - placed by hand through the
//! compound (common TRG 2.3 §13.25 p. 371: a reference point in a compound is
//! local to it).
//!
//! **Tolerance: a micrometre.** Both methods are exact on a circle, so what is
//! left is rounding: angles written to JSON and read back, positions of the
//! order of a Lambert-93 northing (6.86e6 m, whose last digit is ~1e-9 m)
//! differenced and summed over thirty chords.
//!
//! Two frames: one where the compound **is** the projected system - the writer
//! then says grid north - and one offset and turned 30°, where it says plant
//! north (*“a direction in a local engineering coordinate reference system”*,
//! eml 2.3 `NorthReferenceKind`) and the reader must turn the hole with the
//! compound.

use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_eml_2_3 as eml;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_storage::EpcStore;
use energyml_witsml_2_1 as witsml;

const PLANE: &str = "0d0d0d0d-0000-4000-8000-000000000001";
const VERTICAL: &str = "0d0d0d0d-0000-4000-8000-000000000002";
const COMPOUND: &str = "0d0d0d0d-0000-4000-8000-000000000003";

/// A micrometre - see the module.
const TOLERANCE: f64 = 1e-6;

/// The radius of an arc at 15°/30 m: 114.59 m.
fn radius() -> f64 {
    30.0 / 15f64.to_radians()
}

/// Where the compound puts its origin, how far it is turned clockwise from
/// grid north, and which way its vertical axis counts.
#[derive(Clone, Copy)]
struct Frame {
    origin: [f64; 2],
    azimuth: f64,
    /// `Some(origin)`: the axis counts **down** from `origin` metres above the
    /// datum, which counts up (Lutetia's depth compound); `None`: it counts up
    /// from the datum's zero.
    down_from: Option<f64>,
}

impl Frame {
    /// The compound that is Lambert-93 itself.
    fn identity() -> Frame {
        Frame {
            origin: [0.0, 0.0],
            azimuth: 0.0,
            down_from: None,
        }
    }

    /// Offset, turned 30°, counting depths from 35 m above NGF-IGN69.
    fn turned() -> Frame {
        Frame {
            origin: [650_000.0, 6_860_000.0],
            azimuth: 30.0,
            down_from: Some(35.0),
        }
    }

    /// A point on the map, into the frame's local axes - **by hand**, so the
    /// holes are drawn on the map and stored locally. The local x axis is
    /// (cos A, −sin A) on the map and the local y axis (sin A, cos A), for an
    /// azimuth A turned clockwise from grid north; so a map offset (dx, dy)
    /// is x = dx cos A − dy sin A, y = dx sin A + dy cos A. A downward depth
    /// under an origin h above an upward datum is h − elevation.
    fn local(&self, placed: [f64; 3]) -> [f64; 3] {
        let (sin, cos) = self.azimuth.to_radians().sin_cos();
        let (dx, dy) = (placed[0] - self.origin[0], placed[1] - self.origin[1]);
        [
            dx * cos - dy * sin,
            dx * sin + dy * cos,
            match self.down_from {
                Some(h) => h - placed[2],
                None => placed[2],
            },
        ]
    }
}

/// A RESQML wellbore as the boundary knows it: nodes on the map, an elevation
/// each, and the uuids that tie it together.
#[derive(Clone)]
struct Hole {
    name: &'static str,
    frame: &'static str,
    trajectory: &'static str,
    interpretation: &'static str,
    datum: &'static str,
    head: [f64; 3],
    /// Measured depth, and where the node is **on the map** - easting,
    /// northing, elevation.
    nodes: Vec<(f64, [f64; 3])>,
}

/// GTH-3: vertical from its datum at 48 m, a node every 10 m to MD 200.
fn gth3() -> Hole {
    let head = [651_800.0, 6_861_200.0, 48.0];
    Hole {
        name: "GTH-3",
        frame: "33333333-0000-4000-8000-00000000000a",
        trajectory: "33333333-0000-4000-8000-00000000000b",
        interpretation: "33333333-0000-4000-8000-00000000000c",
        datum: "33333333-0000-4000-8000-00000000000d",
        head,
        nodes: (0..=20)
            .map(|k| k as f64 * 10.0)
            .map(|md| (md, [head[0], head[1], head[2] - md]))
            .collect(),
    }
}

/// GTH-3 ST1: leaves GTH-3 at MD 40, builds at 15°/30 m to 45° toward the
/// **north-east** (azimuth 45) - 90 m of arc, to MD 130 - then holds to MD
/// 170. After a turn θ the hole is R (1 − cos θ) along the azimuth and
/// R sin θ down of the kick-off.
fn st1() -> Hole {
    let parent = gth3();
    let r = radius();
    let (s, c) = (45f64.to_radians().sin(), 45f64.to_radians().cos());
    let along = std::f64::consts::FRAC_1_SQRT_2;
    let nodes = (4..=17)
        .map(|k| k as f64 * 10.0)
        .map(|md| {
            let (out, down) = if md <= 130.0 {
                let theta = (md - 40.0) / r;
                (r * (1.0 - theta.cos()), r * theta.sin())
            } else {
                let theta = 90.0 / r;
                (
                    r * (1.0 - theta.cos()) + (md - 130.0) * s,
                    r * theta.sin() + (md - 130.0) * c,
                )
            };
            (
                md,
                [
                    parent.head[0] + out * along,
                    parent.head[1] + out * along,
                    parent.head[2] - 40.0 - down,
                ],
            )
        })
        .collect();
    Hole {
        name: "GTH-3 ST1",
        frame: "31313131-0000-4000-8000-00000000000a",
        trajectory: "31313131-0000-4000-8000-00000000000b",
        interpretation: "31313131-0000-4000-8000-00000000000c",
        datum: parent.datum,
        head: parent.head,
        nodes,
    }
}

/// GTH-2: vertical to MD 30 from its datum at 47 m, an arc at 15°/30 m to the
/// horizontal toward the **west** - 90° is 180 m of hole, to MD 210 - then a
/// drain to MD 300.
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
        frame: "22222222-0000-4000-8000-00000000000a",
        trajectory: "22222222-0000-4000-8000-00000000000b",
        interpretation: "22222222-0000-4000-8000-00000000000c",
        datum: "22222222-0000-4000-8000-00000000000d",
        head,
        nodes,
    }
}

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml::DataObjectReference {
    eml::DataObjectReference {
        uuid: uuid.into(),
        qualified_type: qualified_type.into(),
        title: title.into(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml::Citation {
    eml::Citation {
        title: title.into(),
        originator: "test".into(),
        creation: "2026-09-28T00:00:00Z".into(),
        format: "test".into(),
        ..Default::default()
    }
}

/// A package holding the frame's compound and one datum per head, in the
/// compound's local coordinates.
fn package(frame: Frame, heads: &[&Hole]) -> EpcStore {
    let registry = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "trajectory.epc", registry).expect("an empty package");
    store
        .put(Box::new(eml::LocalEngineering2dCrs {
            uuid: PLANE.into(),
            schema_version: "2.3".into(),
            citation: citation("plane"),
            azimuth: eml::PlaneAngleMeasureExt {
                value: frame.azimuth,
                uom: eml::PlaneAngleUomExt::Dega,
            },
            azimuth_reference: eml::NorthReferenceKind::GridNorth,
            origin_projected_coordinate1: frame.origin[0],
            origin_projected_coordinate2: frame.origin[1],
            horizontal_axes: eml::HorizontalAxes {
                direction1: eml::AxisDirectionKind::East,
                direction2: eml::AxisDirectionKind::North,
                uom: eml::LengthAndTimeUomExt::M,
                is_time: false,
            },
            origin_projected_crs: eml::ProjectedCrs {
                abstract_projected_crs: Some(eml::AbstractProjectedCrs::ProjectedEpsgCrs(
                    eml::ProjectedEpsgCrs { epsg_code: 2154 },
                )),
                ..Default::default()
            },
            ..Default::default()
        }))
        .expect("the plane is a part");
    store
        .put(Box::new(eml::VerticalCrs {
            uuid: VERTICAL.into(),
            schema_version: "2.3".into(),
            citation: citation("NGF-IGN69"),
            direction: eml::VerticalDirection::Up,
            uom: eml::LengthUomExt::M,
            abstract_vertical_crs: Some(eml::AbstractVerticalCrs::VerticalEpsgCrs(eml::VerticalEpsgCrs {
                epsg_code: 5720,
            })),
            ..Default::default()
        }))
        .expect("the vertical datum is a part");
    store
        .put(Box::new(eml::LocalEngineeringCompoundCrs {
            uuid: COMPOUND.into(),
            schema_version: "2.3".into(),
            citation: citation("compound"),
            vertical_crs: dor(VERTICAL, "eml23.VerticalCrs", "NGF-IGN69"),
            origin_vertical_coordinate: frame.down_from.unwrap_or(0.0),
            vertical_axis: eml::VerticalAxis {
                direction: if frame.down_from.is_some() {
                    eml::VerticalDirection::Down
                } else {
                    eml::VerticalDirection::Up
                },
                uom: eml::LengthAndTimeUomExt::M,
                is_time: false,
            },
            local_engineering2d_crs: dor(PLANE, "eml23.LocalEngineering2dCrs", "plane"),
            ..Default::default()
        }))
        .expect("the compound is a part");
    for hole in heads {
        let [x, y, z] = frame.local(hole.head);
        store
            .put(Box::new(eml::ReferencePointInALocalEngineeringCompoundCrs {
                uuid: hole.datum.into(),
                schema_version: "2.3".into(),
                citation: citation(&format!("{} - datum", hole.name)),
                horizontal_coordinates: Some(eml::HorizontalCoordinates {
                    coordinate1: x,
                    coordinate2: y,
                }),
                vertical_coordinate: Some(z),
                crs: dor(COMPOUND, "eml23.LocalEngineeringCompoundCrs", "compound"),
                ..Default::default()
            }))
            .expect("the datum is a part");
    }
    store
}

/// What a frame of the package gives `witsml-wells`: its nodes **in the
/// compound's local axes**, and what the boundary knows of it.
fn frame_of(hole: &Hole, frame: Frame) -> (Source, Known) {
    let points: Vec<[f64; 3]> = hole.nodes.iter().map(|(_, at)| frame.local(*at)).collect();
    let count = points.len() as u32;
    let shape = Source::Shape {
        geometry: Geometry {
            kind: "polyline",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Polylines(vec![count]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let known = Known {
        reference: Some(Ref::new(
            hole.frame,
            "resqml22.WellboreFrameRepresentation",
            format!("{} - frame", hole.name),
        )),
        crs: Some(Ref::new(
            COMPOUND,
            "eml23.LocalEngineeringCompoundCrs",
            "compound",
        )),
        crs_epsg: Some(2154),
        vertical_epsg: Some(5720),
        interpretation: Some(Ref::new(
            hole.interpretation,
            "resqml22.WellboreInterpretation",
            format!("{} - interpretation", hole.name),
        )),
        vertical_uom: Some("m".into()),
        vertical_up: Some(frame.down_from.is_none()),
        node_count: Some(hole.nodes.len()),
        // The datum placed through the compound: where the head is.
        wellhead: Some(hole.head),
        node_mds: Some(hole.nodes.iter().map(|(md, _)| *md).collect()),
        trajectory: Some(Ref::new(
            hole.trajectory,
            "resqml22.WellboreTrajectoryRepresentation",
            format!("{} - trajectory", hole.name),
        )),
        datum: Some(Ref::new(
            hole.datum,
            "eml23.ReferencePointInALocalEngineeringCompoundCrs",
            format!("{} - datum", hole.name),
        )),
        ..Default::default()
    };
    (shape, known)
}

fn bag(holes: &[Hole], frame: Frame) -> Pieces {
    let mut pieces = Pieces::empty();
    for (at, hole) in holes.iter().enumerate() {
        let (shape, known) = frame_of(hole, frame);
        let name = if at == 0 {
            "frames".to_string()
        } else {
            format!("frames {}", at + 1)
        };
        pieces = pieces
            .with(name.clone(), shape)
            .with(format!("{name} (object)"), Source::Object(known));
    }
    pieces
}

fn planned(holes: &[Hole], frame: Frame) -> Plan {
    let when = When {
        creation: "2026-09-28T00:00:00Z".into(),
        originator: "test".into(),
    };
    Registry::new()
        .plan(
            "witsml-wells",
            &bag(holes, frame),
            &Choices::new(),
            &Identity::derived(),
            &when,
        )
        .unwrap_or_else(|why| panic!("witsml-wells plans: {why}"))
}

/// The plan's trajectory of that title, read back from its JSON - the road
/// `apply` takes - and put in the package beside its datum.
fn stored(plan: &Plan, store: &mut EpcStore, title: &str) -> witsml::Trajectory {
    let object: &NewObject = plan
        .objects
        .iter()
        .find(|object| object.qualified_type == "witsml21.Trajectory" && object.title == title)
        .unwrap_or_else(|| panic!("a trajectory titled `{title}`"));
    let schemas = energyml_schemas::all();
    let document =
        energyml_core::json::read_value(&object.json, &schemas).expect("the trajectory reads back");
    let trajectory = document
        .root
        .as_reflect()
        .downcast_ref::<witsml::Trajectory>()
        .expect("a witsml21.Trajectory")
        .clone();
    store
        .put(Box::new(trajectory.clone()))
        .expect("the trajectory is a part");
    trajectory
}

/// Where the reader puts the trajectory, **as the front end places it**: its
/// geometry, then the CRS it is given.
fn drawn(store: &EpcStore, trajectory: &witsml::Trajectory) -> Vec<[f64; 3]> {
    let geometry =
        energyml_geometry::geometry::of(trajectory, store).unwrap_or_else(|why| panic!("drawn: {why}"));
    let crs = energyml_domain::crs::of(trajectory, store)
        .expect("no error")
        .expect("a CRS through its datum");
    geometry
        .patches
        .iter()
        .flat_map(|patch| patch.points.iter())
        .map(|point| crs.to_projected(*point))
        .collect()
}

/// The largest distance between what was drawn and the hand-made hole at the
/// same measured depths - `expected` looked up by MD, which is the one thing
/// both sides state.
fn worst(trajectory: &witsml::Trajectory, found: &[[f64; 3]], expected: &[(f64, [f64; 3])]) -> f64 {
    assert_eq!(
        found.len(),
        trajectory.trajectory_station.len(),
        "a node per station"
    );
    let mut worst: f64 = 0.0;
    for (station, point) in trajectory.trajectory_station.iter().zip(found) {
        let md = station.md.measured_depth.value;
        let (_, at) = expected
            .iter()
            .find(|(depth, _)| *depth == md)
            .unwrap_or_else(|| panic!("a node at MD {md}"));
        let distance =
            ((point[0] - at[0]).powi(2) + (point[1] - at[1]).powi(2) + (point[2] - at[2]).powi(2)).sqrt();
        worst = worst.max(distance);
    }
    worst
}

/// The hole a trajectory's stations run along: the sidetrack's composite runs
/// through its parent above the kick-off.
fn composite(parent: &Hole, own: &Hole) -> Vec<(f64, [f64; 3])> {
    let kickoff = own.nodes[0].0;
    parent
        .nodes
        .iter()
        .filter(|(md, _)| *md < kickoff)
        .chain(own.nodes.iter())
        .copied()
        .collect()
}

fn round_trip(frame: Frame, north: eml::NorthReferenceKind) {
    let holes = [gth3(), st1(), gth2()];
    let plan = planned(&holes, frame);
    let mut store = package(frame, &[&holes[0], &holes[2]]);
    for (title, expected) in [
        ("GTH-3 - trajectory", holes[0].nodes.clone()),
        ("GTH-3 ST1 - trajectory", composite(&holes[0], &holes[1])),
        ("GTH-2 - trajectory", holes[2].nodes.clone()),
    ] {
        let trajectory = stored(&plan, &mut store, title);
        assert_eq!(
            trajectory.azi_ref,
            Some(north.clone()),
            "{title}: the writer's north - the premise of this frame"
        );
        assert!(
            trajectory
                .trajectory_station
                .iter()
                .skip(1)
                .any(|s| s.azi.is_some())
                || title == "GTH-3 - trajectory",
            "{title}: a deviated hole carries azimuths"
        );
        let found = drawn(&store, &trajectory);
        let off = worst(&trajectory, &found, &expected);
        assert!(off <= TOLERANCE, "{title}: {off} m from the hand-made hole");
    }
}

/// The compound is Lambert-93: the writer says grid north, and the stations
/// land on the holes as drawn.
#[test]
fn stations_written_in_grid_north_land_back_on_the_resqml_hole() {
    // To break: the forward method with a wrong ratio factor, the azimuth's
    // east and north swapped, the datum not placed, TVD added rather than
    // subtracted - each moves a deviated hole metres off.
    round_trip(Frame::identity(), eml::NorthReferenceKind::GridNorth);
}

/// The compound is offset, turned 30° and counts depths down from 35 m: the
/// writer says plant north, and the reader must turn the hole with it.
#[test]
fn stations_written_in_plant_north_land_back_on_the_resqml_hole() {
    // To break: plant north read as grid north - GTH-2's 185 m drain would
    // swing 30° and end ~96 m away.
    round_trip(Frame::turned(), eml::NorthReferenceKind::PlantNorth);
}
