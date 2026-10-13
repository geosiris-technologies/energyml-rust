//! `read::wells` — a well read for the section view (ADR-0015).
//!
//! Every object is built in memory after the specification, never read from a
//! package: RESQML TRG 2.2 §12.17 (p. 314) for the trajectory, *"the MdDatum
//! within the MdInterval is mandatory"*, §12.13 (p. 310) for a frame hanging on
//! a trajectory, §12.14 (p. 311) for a wellbore interpretation, §5.34 (p. 130)
//! for `IntervalStratigraphicUnits`. The values are the ones the fixtures give;
//! the shape of each assertion is the specification's.
//!
//! # What it would take to make each one fail
//!
//! - taking the head from the first node: the frame's first node is at MD 10
//!   and the trajectory's at MD 2.5, both under the datum;
//! - keying a well by its own uuid instead of by the wellbore feature: the two
//!   trajectories of Lutetia's shape (a planned and a drilled interpretation of
//!   one feature) would get two keys;
//! - taking any `InterpretedFeature` for a wellbore: one interpretation below
//!   interprets a boundary, whose uuid is not a wellbore;
//! - leaving a silent frame without a reason, or a coloured unit with one it
//!   was not given;
//! - letting a surface through as a well.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::color::{self, Decorated, GraphicalSet, Rgba, Scope};
use energyml_domain::reproject::NoReprojection;
use energyml_domain::source::{ObjectSource, Prefetched};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_section::model::{System, WellKind};
use energyml_section::read::Scene;
use energyml_section::read::wells::{WellContext, read_well, read_well_with};
use energyml_storage::array::ArrayLocation;

const PCRS: &str = "aaaaaaaa-0000-4000-8000-00000000000a";
const VCRS: &str = "22222222-2222-4222-8222-222222222222";
const DATUM: &str = "11111111-1111-4111-8111-111111111111";
/// The wellbore feature, and the planned and drilled interpretations of it.
const WELLBORE: &str = "e0000000-0000-4000-8000-000000000001";
const DRILLED: &str = "e0000000-0000-4000-8000-0000000000a1";
const PLANNED: &str = "e0000000-0000-4000-8000-0000000000a2";
/// A boundary, and an interpretation of it — not a wellbore.
const BOUNDARY: &str = "e0000000-0000-4000-8000-0000000000b0";
const OF_BOUNDARY: &str = "e0000000-0000-4000-8000-0000000000b1";
const RANK: &str = "a0000000-0000-4000-8000-000000000001";
const PRIABONIAN: &str = "b0000000-0000-4000-8000-000000000001";
const BARTONIAN: &str = "b0000000-0000-4000-8000-000000000002";
const SET: &str = "d0000000-0000-4000-8000-000000000001";
/// Trajectories: drilled, planned, one with no interpretation, one that
/// interprets a boundary.
const T_DRILLED: &str = "70000000-0000-4000-8000-0000000000a1";
const T_PLANNED: &str = "70000000-0000-4000-8000-0000000000a2";
const T_ALONE: &str = "70000000-0000-4000-8000-0000000000a3";
const T_ODD: &str = "70000000-0000-4000-8000-0000000000a4";
/// Frames: one stating units, one silent, one on the lone trajectory, one on
/// no trajectory the package holds.
const F_STATING: &str = "50000000-0000-4000-8000-0000000000a1";
const F_SILENT: &str = "50000000-0000-4000-8000-0000000000a2";
const F_ALONE: &str = "50000000-0000-4000-8000-0000000000a3";
const F_LOST: &str = "50000000-0000-4000-8000-0000000000a4";
const SURFACE: &str = "90000000-0000-4000-8000-000000000001";

const RED: Rgba = Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
const BLUE: Rgba = Rgba { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };

// -- building blocks -----------------------------------------------------------

fn dor(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: "x".to_string(),
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

/// The datum of every well here: (1000, 2000), 37.73 above the sea, in a
/// projected system and a vertical one that count up.
fn datum() -> eml23::ReferencePointInACrs {
    eml23::ReferencePointInACrs {
        uuid: DATUM.to_string(),
        vertical_crs: Some(dor(VCRS, "eml23.VerticalCrs")),
        vertical_coordinate: Some(37.73),
        horizontal_coordinates: Some(eml23::AbstractHorizontalCoordinates::ProjectedCoordinates(
            eml23::ProjectedCoordinates {
                coordinate1: 1000.0,
                coordinate2: 2000.0,
                crs: dor(PCRS, "eml23.ProjectedCrs"),
            },
        )),
        ..Default::default()
    }
}

/// A trajectory with no geometry — drilled from its datum, vertically, from MD
/// 2.5 to MD 65 — saying which interpretation it represents.
fn trajectory(uuid: &str, represents: Option<&str>) -> resqml22::WellboreTrajectoryRepresentation {
    resqml22::WellboreTrajectoryRepresentation {
        uuid: uuid.to_string(),
        citation: citation(&format!("{uuid} trajectory")),
        represented_object: represents.map(|uuid| dor(uuid, "resqml22.WellboreInterpretation")),
        md_interval: eml23::MdInterval {
            md_min: 2.5,
            md_max: 65.0,
            uom: eml23::LengthUomExt::M,
            datum: Some(dor(DATUM, "eml23.ReferencePointInACrs")),
            ..Default::default()
        },
        md_domain: Some(resqml22::MdDomain::Driller),
        ..Default::default()
    }
}

fn frame(
    uuid: &str,
    on: &str,
    represents: Option<&str>,
    mds: &[f64],
    maps: Vec<resqml22::IntervalStratigraphicUnits>,
) -> resqml22::WellboreFrameRepresentation {
    resqml22::WellboreFrameRepresentation {
        uuid: uuid.to_string(),
        citation: citation(&format!("{uuid} frame")),
        represented_object: represents.map(|uuid| dor(uuid, "resqml22.WellboreInterpretation")),
        node_count: mds.len() as i64,
        node_md: Some(doubles(mds)),
        trajectory: dor(on, "resqml22.WellboreTrajectoryRepresentation"),
        interval_stratigraphic_units: maps,
        ..Default::default()
    }
}

fn interpretation(uuid: &str, of: &str, feature_type: &str) -> resqml22::WellboreInterpretation {
    resqml22::WellboreInterpretation {
        uuid: uuid.to_string(),
        citation: citation("interpretation"),
        interpreted_feature: dor(of, feature_type),
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
        stratigraphic_organization_interpretation: dor(RANK, "resqml22.StratigraphicColumnRankInterpretation"),
    }
}

fn stratigraphy() -> Vec<Box<dyn DynObject>> {
    let unit = |uuid: &str, title: &str| -> Box<dyn DynObject> {
        Box::new(resqml22::StratigraphicUnitInterpretation {
            uuid: uuid.to_string(),
            citation: citation(title),
            interpreted_feature: dor(BOUNDARY, "resqml22.RockVolumeFeature"),
            ..Default::default()
        })
    };
    vec![
        Box::new(resqml22::StratigraphicColumnRankInterpretation {
            uuid: RANK.to_string(),
            citation: citation("stages"),
            ascending_ordering_criteria: resqml22::OrderingCriteria::Age,
            rank_in_stratigraphic_column: 1,
            stratigraphic_units: [PRIABONIAN, BARTONIAN]
                .iter()
                .map(|uuid| dor(uuid, "resqml22.StratigraphicUnitInterpretation"))
                .collect(),
            interpreted_feature: dor(BOUNDARY, "resqml22.RockVolumeFeature"),
            ..Default::default()
        }),
        unit(PRIABONIAN, "Priabonian"),
        unit(BARTONIAN, "Bartonian"),
    ]
}

/// Priabonian red, Bartonian blue.
fn graphics() -> Vec<(String, GraphicalSet)> {
    let mut set = eml23::GraphicalInformationSet::default();
    for (uuid, rgba) in [(PRIABONIAN, RED), (BARTONIAN, BLUE)] {
        color::set_constant_colour(
            set.as_reflect_mut(),
            &[Decorated {
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

fn nowhere(_: &dyn Reflect) -> Vec<ArrayLocation> {
    Vec::new()
}

/// The package: a wellbore feature with two interpretations (Lutetia's shape,
/// planned and drilled), a trajectory on each, a trajectory that interprets
/// nothing, one that interprets a boundary, and frames on them.
fn package() -> Prefetched {
    let mut objects: Vec<Box<dyn DynObject>> = vec![
        Box::new(eml23::VerticalCrs {
            uuid: VCRS.to_string(),
            direction: eml23::VerticalDirection::Up,
            abstract_vertical_crs: Some(eml23::AbstractVerticalCrs::VerticalEpsgCrs(eml23::VerticalEpsgCrs {
                epsg_code: 5714,
            })),
            ..Default::default()
        }),
        Box::new(eml23::ProjectedCrs {
            uuid: PCRS.to_string(),
            abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedEpsgCrs(eml23::ProjectedEpsgCrs {
                epsg_code: 2154,
            })),
            ..Default::default()
        }),
        Box::new(datum()),
        Box::new(resqml22::WellboreFeature {
            uuid: WELLBORE.to_string(),
            citation: citation("wellbore"),
            ..Default::default()
        }),
        Box::new(interpretation(DRILLED, WELLBORE, "resqml22.WellboreFeature")),
        Box::new(interpretation(PLANNED, WELLBORE, "resqml22.WellboreFeature")),
        Box::new(resqml22::BoundaryFeature {
            uuid: BOUNDARY.to_string(),
            citation: citation("a boundary"),
            ..Default::default()
        }),
        Box::new(interpretation(OF_BOUNDARY, BOUNDARY, "resqml22.BoundaryFeature")),
        Box::new(trajectory(T_DRILLED, Some(DRILLED))),
        Box::new(trajectory(T_PLANNED, Some(PLANNED))),
        Box::new(trajectory(T_ALONE, None)),
        Box::new(trajectory(T_ODD, Some(OF_BOUNDARY))),
        // Priabonian from MD 10 to 20, Bartonian from 20 to 30.
        Box::new(frame(F_STATING, T_DRILLED, Some(DRILLED), &[10.0, 20.0, 30.0], vec![mapping(&[0, 1])])),
        Box::new(frame(F_SILENT, T_DRILLED, Some(DRILLED), &[10.0, 25.0], vec![])),
        Box::new(frame(F_ALONE, T_ALONE, None, &[10.0, 20.0], vec![])),
        Box::new(frame(F_LOST, "99999999-9999-4999-8999-999999999999", None, &[10.0, 20.0], vec![])),
        Box::new(resqml22::TriangulatedSetRepresentation {
            uuid: SURFACE.to_string(),
            citation: citation("a horizon"),
            ..Default::default()
        }),
    ];
    objects.extend(stratigraphy());
    let mut cache = Prefetched::new();
    for object in objects {
        cache.insert(object, &nowhere);
    }
    cache
}

/// The scene's system — that of the objects, so nothing is reprojected.
fn lambert() -> System {
    System { epsg: 2154, unit: "m".to_string() }
}

/// A scene in the objects' own system, with no time: nothing is moved.
#[allow(clippy::result_large_err)]
fn read(uri: &str) -> Result<energyml_section::model::WellRead, energyml_section::model::Refused> {
    let system = lambert();
    let scene = Scene { system: Some(&system), time_factor: None, reprojector: &NoReprojection };
    read_well(&package(), "s1", uri, &scene)
}

#[allow(clippy::result_large_err)]
fn read_with(
    uri: &str,
    context: &WellContext<'_>,
) -> Result<energyml_section::model::WellRead, energyml_section::model::Refused> {
    let system = lambert();
    let scene = Scene { system: Some(&system), time_factor: None, reprojector: &NoReprojection };
    read_well_with(&package(), "s1", uri, &scene, context)
}

fn spans(intervals: &[energyml_section::model::Interval]) -> Vec<(f64, f64, &str, Option<&str>)> {
    intervals.iter().map(|one| (one.from, one.to, one.unit.as_str(), one.colour.as_deref())).collect()
}

// -- the head, the path, the MD --------------------------------------------------

/// The head is the datum — (1000, 2000, 37.73) — **not the first node**: the
/// frame's first node is at MD 10, 10 m below it, and the trajectory's at MD
/// 2.5. Both are collared at the same place, a frame at its trajectory's.
///
/// To make it fail: `head: path[0]` in `read_well_with` — the head drops by 10 m
/// (frame) or 2.5 m (trajectory).
#[test]
fn the_head_is_the_datum_and_not_the_first_node() {
    let frame = read(F_STATING).expect("the frame is read");
    assert_eq!(frame.kind, WellKind::Frame);
    assert_eq!(frame.head, [1000.0, 2000.0, 37.73]);
    assert_eq!(frame.path.len(), 3);
    assert_eq!(frame.path[0], [1000.0, 2000.0, 37.73 - 10.0]);
    assert_eq!(frame.path[2], [1000.0, 2000.0, 37.73 - 30.0]);
    assert_eq!(frame.md, Some(vec![10.0, 20.0, 30.0]));

    let trajectory = read(T_DRILLED).expect("the trajectory is read");
    assert_eq!(trajectory.kind, WellKind::Trajectory);
    assert_eq!(trajectory.head, [1000.0, 2000.0, 37.73]);
    assert_eq!(trajectory.path, vec![[1000.0, 2000.0, 37.73 - 2.5], [1000.0, 2000.0, 37.73 - 65.0]]);
    assert_eq!(trajectory.md, Some(vec![2.5, 65.0]), "a trajectory with no geometry is drawn at its bounds");
    assert_eq!(trajectory.md.as_ref().map(Vec::len), Some(trajectory.path.len()));
    assert_eq!(trajectory.object.uuid, T_DRILLED);
    assert_eq!(trajectory.object.store, "s1");
    assert_eq!(trajectory.object.qualified_type, "resqml22.WellboreTrajectoryRepresentation");
}

// -- one wellbore, one key -------------------------------------------------------

/// A frame and its trajectory are one wellbore, and so are the two
/// trajectories — planned and drilled — of one wellbore feature (see ADR-0015). The key is the **feature's** uuid.
///
/// To make it fail: key = the object's own uuid (the planned and the drilled
/// ones part), or key = the trajectory's uuid (same).
#[test]
fn a_frame_and_its_trajectory_are_one_wellbore() {
    let key = |uri: &str| read(uri).expect("read").wellbore;
    assert_eq!(key(F_STATING).as_deref(), Some(WELLBORE));
    assert_eq!(key(F_SILENT), key(F_STATING));
    assert_eq!(key(T_DRILLED), key(F_STATING));
    assert_eq!(key(T_PLANNED).as_deref(), Some(WELLBORE), "two interpretations, one wellbore feature");
}

/// With no chain the frame takes the key of the trajectory it hangs on — and
/// the trajectory its own uuid — so they are still one line. An interpretation
/// of something that is no wellbore feature is **not** a chain.
///
/// To make it fail: accept any `InterpretedFeature` — `T_ODD` takes the
/// boundary's uuid; or return `None` when there is no chain.
#[test]
fn without_a_chain_a_frame_takes_the_trajectory_it_hangs_on() {
    let alone = read(T_ALONE).expect("read");
    assert_eq!(alone.wellbore.as_deref(), Some(T_ALONE));
    assert_eq!(read(F_ALONE).expect("read").wellbore.as_deref(), Some(T_ALONE));

    let odd = read(T_ODD).expect("read");
    assert_eq!(odd.wellbore.as_deref(), Some(T_ODD), "a boundary is not a wellbore");
    assert_ne!(odd.wellbore.as_deref(), Some(BOUNDARY));
}

// -- the strata ------------------------------------------------------------------

/// A frame that states its units gives them as intervals along its MD — the
/// colour of each from the package's graphics when the caller has them, and
/// `None` (never transparent) when it does not.
///
/// To make it fail: a limit on the other side (`from < md <= to`); or a colour
/// invented when the package gives none.
#[test]
fn a_frame_with_strata_gives_its_intervals() {
    let plain = read(F_STATING).expect("read");
    assert_eq!(
        spans(&plain.intervals),
        [(10.0, 20.0, "Priabonian", None), (20.0, 30.0, "Bartonian", None)],
        "no graphics, no colour: {:?}",
        plain.strata
    );
    assert_eq!(plain.strata, None);

    let sets = graphics();
    let coloured = read_with(F_STATING, &WellContext { graphics: &sets, frames_of: None, index: None, colour_of: None }).expect("read");
    assert_eq!(
        spans(&coloured.intervals),
        [(10.0, 20.0, "Priabonian", Some("#ff0000")), (20.0, 30.0, "Bartonian", Some("#0000ff"))]
    );
}

/// A frame that states nothing says **why** it has no interval — never an
/// empty list that looks like a well without rock. With the frames of its
/// wellbore handed over it is painted from them, by MD (a node opens the
/// interval: node 10 is Priabonian, node 25 Bartonian), and the same
/// trajectory is painted at its bounds; with other frames it is still refused.
///
/// To make it fail: return `Vec::new()` and `None` for a silent frame.
#[test]
fn a_silent_frame_says_why_it_has_no_interval() {
    let silent = read(F_SILENT).expect("read");
    assert!(silent.intervals.is_empty());
    let why = silent.strata.expect("a reason");
    assert!(why.contains("frame") && why.contains("not consulted"), "{why}");

    let trajectory = read(T_DRILLED).expect("read");
    assert!(trajectory.intervals.is_empty() && trajectory.strata.is_some());

    // The frames of the wellbore, as a face that holds the package index gives them.
    let sisters = |wellbore: &str| -> Vec<Box<dyn DynObject>> {
        assert_eq!(wellbore, DRILLED, "asked for the wellbore the object represents");
        let source = package();
        vec![source.object(F_STATING).expect("held")]
    };
    let sets = graphics();
    let context = WellContext { graphics: &sets, frames_of: Some(&sisters), index: None, colour_of: None };
    let carried = read_with(F_SILENT, &context).expect("read");
    assert_eq!(carried.strata, None);
    // The stating frame's rock between the silent frame's nodes: Priabonian to
    // 20, Bartonian below — not the Priabonian of node 10 stretched to node 25,
    // which this said until 03/10 (`strata_by_md::Painting::intervals`).
    assert_eq!(
        spans(&carried.intervals),
        [
            (10.0, 20.0, "Priabonian", Some("#ff0000")),
            (20.0, 25.0, "Bartonian", Some("#0000ff")),
            (25.0, 25.0, "Bartonian", Some("#0000ff"))
        ]
    );
    assert!(carried.borrowed, "painted from its sisters");
}

// -- what is not a well ----------------------------------------------------------

/// An object that is no well is refused **by its type**, one that is not in
/// the package by its uri, and a frame whose trajectory is absent by that
/// trajectory — each carries the object it was asked for, so the lot goes on.
///
/// To make it fail: let another type through to `head_of`.
#[test]
fn what_is_no_well_is_refused_by_name() {
    let surface = read(SURFACE).expect_err("a surface is no well");
    assert!(surface.reason.contains("TriangulatedSetRepresentation"), "{}", surface.reason);
    assert_eq!(surface.object.uuid, SURFACE);
    assert_eq!(surface.object.store, "s1");

    let absent = read("nope").expect_err("not in the package");
    assert!(absent.reason.contains("nope"), "{}", absent.reason);
    assert_eq!(absent.object.uri, "nope");

    let lost = read(F_LOST).expect_err("its trajectory is not held");
    // The trajectory is named by the uri of the reference, whose uuid is in it.
    assert!(
        lost.reason.contains("trajectory eml:///") && lost.reason.contains("99999999-9999-4999-8999-999999999999"),
        "{}",
        lost.reason
    );
    assert_eq!(lost.object.uuid, F_LOST);
}

// -- a trajectory held in two versions ---------------------------------------------

/// A frame whose trajectory reference names **version 2**, over a source that
/// holds that trajectory twice: asked its uuid it gives version 1 (which
/// represents nothing), only a uri naming `version='2'` gives version 2 (which
/// represents the drilled interpretation).
struct TwoVersions<'a> {
    inner: &'a Prefetched,
    frame: resqml22::WellboreFrameRepresentation,
    second: resqml22::WellboreTrajectoryRepresentation,
}

const F_VERSIONED: &str = "50000000-0000-4000-8000-0000000000a5";

impl ObjectSource for TwoVersions<'_> {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        if uuid == F_VERSIONED { Some(Box::new(self.frame.clone())) } else { self.inner.object(uuid) }
    }

    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        match energyml_core::identifiers::EtpUri::parse(uri) {
            Ok(parsed) if parsed.uuid.as_deref() == Some(T_ALONE) && parsed.object_version.as_deref() == Some("2") => {
                Some(Box::new(self.second.clone()))
            }
            Ok(_) => self.inner.object_at(uri),
            Err(_) => self.object(uri),
        }
    }
}

impl energyml_domain::source::ArraySource for TwoVersions<'_> {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        self.inner.locate(object)
    }
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<energyml_storage::array::ArrayMetadata> {
        self.inner.metadata(location)
    }
    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<energyml_storage::array::ArrayData> {
        self.inner.read(location)
    }
    fn read_into(
        &self,
        location: &ArrayLocation,
        out: &mut energyml_storage::array::ArrayBuffer<'_>,
    ) -> energyml_domain::Result<energyml_storage::array::ArrayMetadata> {
        self.inner.read_into(location, out)
    }
}

/// A frame hangs on the trajectory **version** its reference names, and so
/// belongs to that version's wellbore: version 2 represents the drilled
/// interpretation (its wellbore is the feature), version 1 represents nothing
/// (its wellbore would be the trajectory's own uuid).
///
/// Internally only the full uri is used (ADR-0003).
///
/// To make it fail: in `wells::trajectory_of_frame`, ask `source.object(uuid)` — the
/// wellbore comes out as `T_ALONE`.
#[test]
fn a_frame_belongs_to_the_wellbore_of_the_trajectory_version_it_names() {
    let inner = package();
    let mut frame = frame(F_VERSIONED, T_ALONE, None, &[10.0, 20.0], vec![]);
    frame.trajectory.object_version = Some("2".to_string());
    let source = TwoVersions { inner: &inner, frame, second: trajectory(T_ALONE, Some(DRILLED)) };
    let system = lambert();
    let scene = Scene { system: Some(&system), time_factor: None, reprojector: &NoReprojection };
    let read = read_well(&source, "s1", F_VERSIONED, &scene).expect("reads");
    assert_eq!(read.wellbore.as_deref(), Some(WELLBORE), "version 2 represents the drilled interpretation");
}
