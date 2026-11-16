//! The one table of a hexahedron's faces, and the one expansion of closed
//! polyline runs (`[export-providers-cache-rules S10]`), on a unit cube whose
//! answers can be stated.

use energyml_geometry::shape::{HEXAHEDRON_FACES, polyline_runs};

/// VTK's hexahedron: the face 0-1-2-3 counter-clockwise seen from above, 4-7
/// the top face above it.
const CUBE: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0],
];

/// Every face's right-hand normal points out of the cell: against the vector
/// from the cube's centre to the face's own, the dot product is positive; and
/// every corner is on exactly three faces.
#[test]
fn every_face_is_wound_outward() {
    let centre = [0.5, 0.5, 0.5];
    let mut seen = [0usize; 8];
    for face in HEXAHEDRON_FACES {
        let [a, b, c, d] = face.map(|corner| CUBE[corner]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let middle = [
            (a[0] + b[0] + c[0] + d[0]) / 4.0 - centre[0],
            (a[1] + b[1] + c[1] + d[1]) / 4.0 - centre[1],
            (a[2] + b[2] + c[2] + d[2]) / 4.0 - centre[2],
        ];
        let out = normal[0] * middle[0] + normal[1] * middle[1] + normal[2] * middle[2];
        assert!(out > 0.0, "face {face:?} points inward ({out})");
        for corner in face {
            seen[corner] += 1;
        }
    }
    assert_eq!(seen, [3; 8]);
}

/// The skin of two positively oriented cells side by side: ten faces, each
/// wound outward (its right-hand normal points away from the block's centre),
/// in the order the cells and the face table give them, on every run
/// (`[geometry G2]`).
/// Break it: wind each boundary ring by its XY area again (the bottom ring then
/// faces up, the first ring is not `[0, 3, 2, 1]`), or collect the faces from
/// the values of a `HashMap` (the order differs between the repeated calls).
#[test]
fn the_skin_of_a_grid_is_wound_outward_and_ordered() {
    use energyml_geometry::geometry::{Geometry, Patch, Topology};
    use energyml_geometry::shape::Shape;

    // 3 x 2 x 2 nodes, index = x + 3 y + 6 z.
    let mut points = Vec::new();
    for z in [0.0_f64, 1.0] {
        for y in [0.0_f64, 1.0] {
            for x in [0.0_f64, 1.0, 2.0] {
                points.push([x, y, z]);
            }
        }
    }
    // VTK order: the bottom face counter-clockwise seen from above, then the top.
    let cell = |l: u32| [l, l + 1, l + 4, l + 3, l + 6, l + 7, l + 10, l + 9];
    let skin = || {
        let geometry = Geometry {
            kind: "test",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: points.clone(),
                topology: Topology::Hexahedra {
                    cells: vec![cell(0), cell(1)],
                    dimensions: [2, 1, 1],
                    index: Vec::new(),
                },
                closed: Vec::new(),
            }],
        };
        match geometry
            .as_shape(Shape::Surface)
            .expect("a skin")
            .patches
            .remove(0)
            .topology
        {
            Topology::Quads(rings) => rings,
            other => panic!("a skin is quadrilaterals, not {}", other.describe()),
        }
    };

    let rings = skin();
    assert_eq!(rings.len(), 10, "two cells in a row: 12 faces, 2 shared");
    // First-seen order: the first face of the first cell is its bottom one,
    // [0, 3, 2, 1] in cell corners, i.e. nodes [0, 3, 4, 1].
    assert_eq!(rings[0], [0, 3, 4, 1]);
    for _ in 0..16 {
        assert_eq!(skin(), rings, "the order of the faces changed between two calls");
    }
    let centre = [1.0, 0.5, 0.5];
    for ring in &rings {
        let [a, b, c, d] = ring.map(|corner| points[corner as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let out = (0..3)
            .map(|k| normal[k] * ((a[k] + b[k] + c[k] + d[k]) / 4.0 - centre[k]))
            .sum::<f64>();
        assert!(out > 0.0, "ring {ring:?} points inward ({out})");
    }
}

/// A run of fewer than two nodes is dropped, a run the file states closed ends
/// on its first node, and an absent flag states nothing.
/// Break it: push the first node for a run with `closed[rank]` false, or keep
/// the runs of one node, and the expected lists change.
#[test]
fn polyline_runs_close_only_what_the_file_closes() {
    let runs = [3, 1, 2, 4];
    assert_eq!(
        polyline_runs(&runs, &[true, false, false, true]),
        vec![vec![0, 1, 2, 0], vec![4, 5], vec![6, 7, 8, 9, 6]]
    );
    // No `ClosedPolylines`: nothing is a ring.
    assert_eq!(
        polyline_runs(&runs, &[]),
        vec![vec![0, 1, 2], vec![4, 5], vec![6, 7, 8, 9]]
    );
}
