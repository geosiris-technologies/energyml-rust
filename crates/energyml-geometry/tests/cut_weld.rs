//! Two neighbouring triangles put their shared edge's intersection on the same
//! point, to the bit, whatever their winding: the chain of a cut through a
//! mesh that is not aligned on the axes is one polyline, not fragments.
//!
//! Every test of the cut before this one used a mesh on round coordinates,
//! where `a + (b - a) * t` and `b + (a - b) * t'` happen to agree; on a real
//! mesh they differ in the last bits, and a weld on exact ends then breaks.

use energyml_geometry::section::{chain, chain_sourced, cut, cut_sourced};

/// A 24 x 24 grid of nodes on coordinates no binary fraction writes exactly,
/// gently disturbed, with a sloping and curved height; each cell is two
/// triangles wound the same way, so a shared edge is walked one way by one
/// triangle and the other way by its neighbour.
fn oblique_mesh() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let n = 24_u32;
    let mut points = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (fi, fj) = (f64::from(i), f64::from(j));
            let x = fi * 1.37 + 0.11 * (fj * 0.7).sin();
            let y = fj * 1.29 + 0.13 * (fi * 0.9).cos();
            let z = 0.31 * x - 0.17 * y + 0.05 * (x * y).sqrt();
            points.push([x, y, z]);
        }
    }
    let mut triangles = Vec::new();
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let (p00, p10) = (j * n + i, j * n + i + 1);
            let (p01, p11) = ((j + 1) * n + i, (j + 1) * n + i + 1);
            triangles.push([p00, p10, p11]);
            triangles.push([p00, p11, p01]);
        }
    }
    (points, triangles)
}

/// A straight oblique trace that enters and leaves the mesh once.
const TRACE: [[f64; 2]; 2] = [[-2.3, 1.7], [33.9, 27.1]];

#[test]
fn a_cut_through_an_oblique_mesh_is_one_polyline() {
    let (points, triangles) = oblique_mesh();
    let cuts = cut(&points, &triangles, &TRACE);
    assert!(
        cuts.len() > 40,
        "the trace crosses many triangles: {}",
        cuts.len()
    );
    let chains = chain(&cuts);
    assert_eq!(
        chains.len(),
        1,
        "one crossing of a connected mesh is one polyline, got {} pieces",
        chains.len()
    );
    assert!(!chains[0].closed);
    // Every segment is in it: n segments welded end to end give n + 1 nodes.
    assert_eq!(chains[0].points.len(), cuts.len() + 1);
    // Break it: interpolate each edge in the triangle's own order of corners in
    // `cut_sourced` (section.rs), instead of from its lower node index.
}

#[test]
fn the_sourced_chain_welds_the_same_way() {
    let (points, triangles) = oblique_mesh();
    let cuts = cut_sourced(&points, &triangles, &TRACE);
    let chains = chain_sourced(&cuts);
    assert_eq!(chains.len(), 1, "{} pieces", chains.len());
    // Break it: same as above.
}
