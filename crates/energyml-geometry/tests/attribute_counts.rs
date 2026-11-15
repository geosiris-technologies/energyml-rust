//! `scene::attributes_for` lines a property up through the one count rule
//! (`energyml_domain::property::count_fit`): *"The size of the vector is
//! constant on all elements"*, `ValueCountPerIndexableElement` being the count
//! per element (RESQML TRG 2.2 §7.1 p. 222).
//!
//! Built by hand, no file read. The three cases of the rule, seen from the
//! scene: a count that fits, a count that holds one value per element where
//! several are declared (attached as scalars, and said), and anything else
//! (refused, with its numbers).

use energyml_domain::source::NoObjects;
use energyml_eml_2_3::{DataObjectReference, FloatingPointXmlArray, IndexableElement};
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_geometry::scene::{self, SceneObject};
use energyml_resqml_2_2::{AbstractValueArray, ContinuousProperty};

const OBJECT: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

/// A point set of three nodes, with no CRS.
fn point_set() -> SceneObject {
    SceneObject {
        uuid: OBJECT.to_string(),
        uri: format!("eml:///resqml22.PointSetRepresentation({OBJECT})"),
        title: String::new(),
        qualified_type: "resqml22.PointSetRepresentation".to_string(),
        geometry: Geometry {
            kind: "PointSetRepresentation",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0; 3]; 3],
                topology: Topology::Points,
                closed: vec![],
            }],
        },
        crs: None,
        interpretation: None,
        feature: None,
        metadata: Default::default(),
        attributes: Vec::new(),
    }
}

/// A property on the nodes that declares two values per node and holds `held`.
fn property(held: usize) -> ContinuousProperty {
    ContinuousProperty {
        uuid: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".to_string(),
        supporting_representation: DataObjectReference {
            uuid: OBJECT.to_string(),
            ..Default::default()
        },
        indexable_element: IndexableElement::Nodes,
        value_count_per_indexable_element: vec![2],
        values_for_patch: vec![AbstractValueArray::FloatingPointXmlArray(FloatingPointXmlArray {
            count_per_value: 1,
            values: (0..held).map(|i| i as f64).collect(),
            ..Default::default()
        })],
        ..Default::default()
    }
}

/// Three values for three nodes of two declared components: one per node, so
/// the declaration is what is wrong. Breaking it: treat `OnePerElement` as a
/// mismatch in `scene.rs`, the property is then refused.
#[test]
fn one_value_per_node_where_two_are_declared_is_attached_as_scalars_and_said() {
    let (found, notes) = scene::attributes_for(&point_set(), &[&property(3)], &NoObjects);
    assert_eq!(found.len(), 1, "attached, not refused: {notes:?}");
    assert_eq!(found[0].components, 1);
    assert!(
        notes.iter().any(|note| note.contains("holds exactly one")),
        "the disagreement is said: {notes:?}"
    );
}

/// Six values for three nodes of two components: vectors, nothing to say.
/// Breaking it: always demote to one component, `components` is then 1.
#[test]
fn six_values_for_three_nodes_of_two_components_attach_as_vectors() {
    let (found, notes) = scene::attributes_for(&point_set(), &[&property(6)], &NoObjects);
    assert_eq!(found.len(), 1, "{notes:?}");
    assert_eq!(found[0].components, 2);
    assert!(notes.is_empty(), "{notes:?}");
}

/// Five values for three nodes of two components fit nothing: refused, with the
/// numbers. Breaking it: attach whatever the count, the property is then found.
#[test]
fn five_values_for_three_nodes_of_two_components_are_refused() {
    let (found, notes) = scene::attributes_for(&point_set(), &[&property(5)], &NoObjects);
    assert!(
        found.is_empty(),
        "five values on three nodes of two must not attach"
    );
    assert!(
        notes
            .iter()
            .any(|note| note.contains("has 5 value(s) for 3 point(s) × 2")),
        "{notes:?}"
    );
}
