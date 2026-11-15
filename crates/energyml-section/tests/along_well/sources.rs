//! `along_well::sources`: the registry of the sources of a curtain, and the cube on a frame.
//!
//! RESQML TRG 2.2 section 7.1 p. 222: the value count per element is given slowest dimension first, and
//! the 2.0.1 `Count` is one number, whatever it covers: a flat list of components is never factorised.

use energyml_core::registry::DynObject;
use energyml_domain::ArraySource;
use energyml_domain::property::shape::ComponentPick;
use energyml_domain::series::matrix::Plane;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_section::along_well::{
    AlongWellRefusal, AxisSignAsk, CurtainAsk, CurtainColumns, CurtainSource, CurtainSources, Note, PlaneAxisAsk,
    place_plane, plane_of_roles,
};

use super::{FRAME, Package, frame_uri, uri};

const UNBUILT: &str = "a0000000-0000-4000-8000-0000000000b1";
const ONE: &str = "a0000000-0000-4000-8000-0000000000b2";
const CUBE: &str = "a0000000-0000-4000-8000-0000000000b3";
const FLAT: &str = "a0000000-0000-4000-8000-0000000000b4";
const MATRIX: &str = "a0000000-0000-4000-8000-0000000000b5";

fn property(uuid: &str, counts: &[i64]) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: uuid.to_string(),
        citation: eml23::Citation {
            title: format!("property {}", &uuid[uuid.len() - 2..]),
            ..Default::default()
        },
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: counts.to_vec(),
        property_kind: eml23::DataObjectReference {
            uuid: "d0000000-0000-4000-8000-000000000001".to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: "a kind".to_string(),
            ..Default::default()
        },
        supporting_representation: eml23::DataObjectReference {
            uuid: FRAME.to_string(),
            qualified_type: "resqml22.WellboreFrameRepresentation".to_string(),
            title: "a frame".to_string(),
            ..Default::default()
        },
        uom: eml23::UnitOfMeasureExt::from_xml_text("ohm.m").expect("a unit"),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerConstantArray(
            eml23::IntegerConstantArray {
                value: 1,
                count: 3,
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

fn package() -> Package {
    let boxed = |object: resqml22::ContinuousProperty| -> Box<dyn DynObject> { Box::new(object) };
    Package(vec![
        boxed(property(UNBUILT, &[1])),
        boxed(property(ONE, &[1])),
        boxed(property(CUBE, &[3, 3, 3])),
        boxed(property(FLAT, &[9])),
        boxed(property(MATRIX, &[3, 2])),
    ])
}

fn roles() -> [(PlaneAxisAsk, AxisSignAsk); 2] {
    [
        (PlaneAxisAsk::HighSide, AxisSignAsk::Positive),
        (PlaneAxisAsk::Right, AxisSignAsk::Positive),
    ]
}

fn ask(uuid: &str) -> CurtainAsk {
    CurtainAsk {
        property: uri("resqml22.ContinuousProperty", uuid),
        source: "cube-on-frame".to_string(),
        links: Vec::new(),
        roles: roles(),
        lateral_index: 0,
    }
}

/// A source that is declared and not built is refused by name, never an empty curtain (ADR-0005).
///
/// To break it: return empty columns from `NotBuilt`.
#[test]
fn an_unbuilt_source_is_refused_never_empty() {
    let sources = CurtainSources::builtin();
    let found = sources
        .get("grid-property")
        .expect("declared")
        .columns(&ask(UNBUILT), &frame_uri(), 4, &package());
    assert!(matches!(
        found,
        Err(AlongWellRefusal::SourceNotBuilt { id: "grid-property" })
    ));
    let found = sources
        .get("lwd-image")
        .expect("declared")
        .columns(&ask(UNBUILT), &frame_uri(), 4, &package());
    assert!(matches!(found, Err(AlongWellRefusal::SourceNotBuilt { id: "lwd-image" })));
    assert_eq!(sources.ids(), vec!["cube-on-frame", "grid-property", "lwd-image"]);
}

struct Mine;

impl CurtainSource for Mine {
    fn id(&self) -> &'static str {
        "cube-on-frame"
    }

    fn columns(
        &self,
        _ask: &CurtainAsk,
        _frame_uri: &str,
        _node_count: u64,
        _source: &dyn ArraySource,
    ) -> Result<CurtainColumns, AlongWellRefusal> {
        Ok(CurtainColumns {
            property: "mine".to_string(),
            values: Vec::new(),
            rows_up: Vec::new(),
            lateral: 0.0,
            unit: None,
            notes: Vec::new(),
        })
    }
}

/// The caller's source is tried before the built-in one of the same id (ADR-0006).
///
/// To break it: append the caller's source instead of inserting it first (the built-in one answers, and
/// refuses the property that is not on this package).
#[test]
fn the_callers_source_is_tried_first() {
    let sources = CurtainSources::builtin().with(Box::new(Mine));
    assert_eq!(
        sources.ids(),
        vec!["cube-on-frame", "cube-on-frame", "grid-property", "lwd-image"]
    );
    let answered = sources
        .get("cube-on-frame")
        .unwrap()
        .columns(&ask(UNBUILT), &frame_uri(), 4, &package())
        .unwrap();
    assert_eq!(answered.property, "mine");
}

/// A curtain is a matrix of two dimensions per node, each at least 2: `[1]` is a scalar, `[3, 3, 3]` is
/// three-dimensional, and a 2.0.1 `Count = 9` is a flat list that is **never** factorised into 3 x 3
/// (RESQML TRG 2.2 section 7.1 p. 222).
///
/// To break it: factorise 9 into 3 x 3 (the third answer is a curtain).
#[test]
fn a_matrix_that_is_not_two_dimensional_is_refused() {
    let cube = CurtainSources::builtin();
    let cube = cube.get("cube-on-frame").unwrap();
    let on = |uuid: &str| cube.columns(&ask(uuid), &frame_uri(), 4, &package());
    assert!(matches!(
        on(ONE),
        Err(AlongWellRefusal::NotTwoDimensions { counts }) if counts == vec![1_u64]
    ));
    assert!(matches!(
        on(CUBE),
        Err(AlongWellRefusal::NotTwoDimensions { counts }) if counts == vec![3_u64, 3, 3]
    ));
    assert!(matches!(
        on(FLAT),
        Err(AlongWellRefusal::FlatComponents { count: 9 })
    ));
}

/// A property on another support than the frame is not a curtain of this frame.
///
/// To break it: skip the check of the support.
#[test]
fn a_property_on_another_support_is_refused() {
    let sources = CurtainSources::builtin();
    let cube = sources.get("cube-on-frame").unwrap();
    let other = uri(
        "resqml22.WellboreFrameRepresentation",
        "f0000000-0000-4000-8000-0000000000ff",
    );
    let found = cube.columns(&ask(MATRIX), &other, 4, &package());
    assert!(matches!(found, Err(AlongWellRefusal::NotOnFrame { .. })));
}

/// With no axis declared and no convention B facet in the file, the two dimensions are known by their
/// ranks only, and a rank is not an offset: the curtain is refused, naming the dimensions (Q3).
///
/// To break it: draw on the ranks.
#[test]
fn ranks_are_not_offsets() {
    let sources = CurtainSources::builtin();
    let found = sources
        .get("cube-on-frame")
        .unwrap()
        .columns(&ask(MATRIX), &frame_uri(), 4, &package());
    assert!(matches!(
        found,
        Err(AlongWellRefusal::AxesUnknown { dims }) if dims == vec![0_usize, 1]
    ));
}

/// The dimension along the high side and its sign are the caller's declaration (S2), not read from the
/// file: with `(HighSide, Negative)` first the rows are the offsets of dimension 0 with their sign changed,
/// and the plane is the pick `Along { axis: 0 }`; with the roles swapped it is dimension 1.
///
/// To break it: take dimension 0 without reading the roles (the second half fails).
#[test]
fn the_high_side_dimension_and_its_sign_are_the_callers() {
    let declared = [
        (PlaneAxisAsk::HighSide, AxisSignAsk::Negative),
        (PlaneAxisAsk::Right, AxisSignAsk::Positive),
    ];
    let (h, l, pick) = plane_of_roles(&declared, 1, [3, 2]).unwrap();
    assert_eq!((h, l), (0, 1));
    assert_eq!(pick, ComponentPick::Along { axis: 0, at: vec![0, 1] });
    let mut wanted = ask(MATRIX);
    wanted.roles = declared;
    wanted.lateral_index = 1;
    let plane = Plane {
        rows: 4,
        cols: 3,
        pick,
        values: (0..12).map(f64::from).collect(),
        absent: 0,
        notes: Vec::new(),
    };
    let offsets = [vec![-10.0, 0.0, 10.0], vec![-1.0, 1.0]];
    let columns = place_plane(&wanted, &offsets, h, l, plane.clone(), Some("ohm.m".to_string()));
    assert_eq!(columns.rows_up, vec![10.0, 0.0, -10.0]);
    assert_eq!(columns.lateral, 1.0);
    assert_eq!(columns.values, plane.values);
    assert_eq!(columns.unit.as_deref(), Some("ohm.m"));
    assert_eq!(columns.notes, vec![Note::RolesDeclared]);

    let swapped = [
        (PlaneAxisAsk::Right, AxisSignAsk::Positive),
        (PlaneAxisAsk::HighSide, AxisSignAsk::Negative),
    ];
    let (h, l, pick) = plane_of_roles(&swapped, 0, [3, 2]).unwrap();
    assert_eq!((h, l), (1, 0));
    assert_eq!(pick, ComponentPick::Along { axis: 1, at: vec![0, 0] });
    let mut wanted = ask(MATRIX);
    wanted.roles = swapped;
    let plane = Plane {
        rows: 4,
        cols: 2,
        pick,
        values: (0..8).map(f64::from).collect(),
        absent: 0,
        notes: Vec::new(),
    };
    let columns = place_plane(&wanted, &offsets, h, l, plane, None);
    assert_eq!(columns.rows_up, vec![1.0, -1.0]);
    assert_eq!(columns.lateral, -10.0);
}

/// A lateral index outside its dimension, and two dimensions along one axis, are refused by name.
///
/// To break it: clamp the index.
#[test]
fn a_lateral_index_out_of_range_and_one_axis_twice_are_refused() {
    assert!(matches!(
        plane_of_roles(&roles(), 2, [3, 2]),
        Err(AlongWellRefusal::LateralIndex { index: 2, len: 2 })
    ));
    let twice = [
        (PlaneAxisAsk::HighSide, AxisSignAsk::Positive),
        (PlaneAxisAsk::HighSide, AxisSignAsk::Negative),
    ];
    assert!(matches!(
        plane_of_roles(&twice, 0, [3, 2]),
        Err(AlongWellRefusal::Grid(_))
    ));
}
