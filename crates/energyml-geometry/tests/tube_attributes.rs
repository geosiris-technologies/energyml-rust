//! A coloured tube keeps each node's value on that node's ring, even when the
//! sweep dropped a station (`[geometry G3]`), and a polyline run that names
//! more nodes than the patch holds is refused by name (`[geometry G9]`).

use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_geometry::scene::{Attribute, AttributeScope, SceneObject};
use energyml_geometry::tube::{Tube, sweep_object};

fn well(points: Vec<[f64; 3]>, lengths: Vec<u32>, values: Vec<f64>) -> SceneObject {
    SceneObject {
        uuid: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".to_string(),
        uri: "eml:///resqml22.PolylineSetRepresentation(cccccccc-cccc-4ccc-8ccc-cccccccccccc)".to_string(),
        title: String::new(),
        qualified_type: "resqml22.PolylineSetRepresentation".to_string(),
        geometry: Geometry {
            kind: "PolylineSetRepresentation",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Polylines(lengths),
                closed: Vec::new(),
            }],
        },
        crs: None,
        interpretation: None,
        feature: None,
        metadata: Default::default(),
        attributes: vec![Attribute {
            uuid: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".to_string(),
            name: "md".to_string(),
            kind: None,
            uom: None,
            scope: AttributeScope::Points,
            components: 1,
            values,
            lut: Vec::new(),
        }],
    }
}

/// Node 1 repeats node 0, so the sweep keeps nodes 0, 2 and 3: the ring of node
/// 2 must carry 12.0, not the 11.0 of the dropped node that sits at its index in
/// the filtered list.
/// Break it: make `distinct` return no origin and push `index as u32` in
/// `station_of` again: the second ring then carries 11.0 and the third 12.0.
#[test]
fn a_dropped_station_does_not_shift_the_values_of_the_rings_after_it() {
    let object = well(
        vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, -1.0],
            [0.0, 0.0, -2.0],
        ],
        vec![4],
        vec![10.0, 11.0, 12.0, 13.0],
    );
    let tube = Tube::checked(1.0, 4).unwrap();
    let swept = sweep_object(&object, &tube).expect("a tube");
    let values = &swept.attributes[0].values;
    assert_eq!(values.len(), 3 * 4, "three stations, four vertices each");
    for (ring, expected) in [10.0, 12.0, 13.0].into_iter().enumerate() {
        assert!(
            values[ring * 4..(ring + 1) * 4].iter().all(|v| *v == expected),
            "ring {ring} should carry {expected}: {values:?}"
        );
    }
}

/// A run list that sums to more nodes than the patch has: a named refusal, not
/// an index out of range.
/// Break it: remove the guard at the top of the patch loop in `sweep_object`:
/// the call panics.
#[test]
fn runs_past_the_points_are_refused_by_name() {
    let object = well(
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, -2.0]],
        vec![5],
        vec![1.0, 2.0, 3.0],
    );
    let refused = sweep_object(&object, &Tube::checked(1.0, 4).unwrap()).expect_err("5 nodes of 3");
    assert!(refused.to_string().contains("node 3"), "{refused}");
}
