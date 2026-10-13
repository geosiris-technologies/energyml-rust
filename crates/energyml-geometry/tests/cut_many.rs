//! `section::cut_many` against `section::cut`, trace by trace.
//!
//! The contract is two sentences: each trace's answer is `cut`'s own, **to the
//! bit**, and the box prefilter spares work without losing a segment. The
//! surface is fabricated (an undulating sheet over a regular lattice, with a
//! holed corner and a triangle indexing past the points), because the corpus
//! holds no surface whose every triangle is known.

use energyml_geometry::section::{self, Cut, cut, cut_many, cut_many_counted};

/// An `n × n` lattice of unit cells over (0, 0)..(n, n), two triangles a cell,
/// on `z = 10 + 3 sin(x/2) + 2 cos(y/3)`: a surface that is neither a plane nor
/// symmetric, so a section along it has bends to get wrong.
fn sheet(n: usize) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let mut points = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (i as f64, j as f64);
            points.push([x, y, 10.0 + 3.0 * (x / 2.0).sin() + 2.0 * (y / 3.0).cos()]);
        }
    }
    let at = |i: usize, j: usize| (j * (n + 1) + i) as u32;
    let mut triangles = Vec::new();
    for j in 0..n {
        for i in 0..n {
            triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    (points, triangles)
}

/// Every number of a cut, as bits: `==` on `f64` would let `-0.0` and `0.0`
/// pass for each other, and the claim is *to the bit*.
fn bits(cuts: &[Cut]) -> Vec<u64> {
    cuts.iter()
        .flat_map(|c| {
            c.ends
                .iter()
                .flatten()
                .chain(c.profile.iter().flatten())
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Several traces - a bend, a crossing one, one beside the sheet, one with a
/// repeated point (a segment of no length) - each equals `cut`'s answer.
///
/// To make it fail: in `cut_many`, forget the `offset` added to the profile -
/// the bent trace's second segment starts again at s = 0; or shrink the margin
/// to 0 and put a trace exactly along a lattice line.
#[test]
fn cut_many_is_cut_trace_by_trace_to_the_bit() {
    let (mut points, mut triangles) = sheet(20);
    // A holed corner and an index past the points: `cut` drops both, so must the
    // prefilter - without it the box of a NaN triangle would be NaN.
    points[45][2] = f64::NAN;
    triangles.push([0, 1, 9999]);
    let traces: Vec<Vec<[f64; 2]>> = vec![
        vec![[-3.0, 4.2], [8.0, 4.2], [8.0, 15.5], [19.0, 12.0]],
        vec![[2.5, -1.0], [2.5, 25.0]],
        vec![[50.0, 50.0], [60.0, 61.0]],
        vec![[1.0, 1.0], [1.0, 1.0], [12.0, 7.0]],
        // Exactly along a lattice line: every hit is on a shared edge.
        vec![[0.0, 6.0], [20.0, 6.0]],
    ];
    let many = cut_many(&points, &triangles, &traces);
    assert_eq!(many.len(), traces.len());
    for (at, trace) in traces.iter().enumerate() {
        let alone = cut(&points, &triangles, trace);
        assert_eq!(bits(&many[at]), bits(&alone), "trace {at}");
        assert_eq!(many[at].len(), alone.len(), "trace {at}");
    }
    // The answer is not trivially empty: the crossing traces cut something, and
    // the one beside the sheet nothing.
    assert!(!many[0].is_empty() && !many[1].is_empty() && !many[4].is_empty());
    assert!(many[2].is_empty());
    // And the welding downstream is the same too: a chain is a function of the cuts.
    assert_eq!(
        section::chain(&many[0]).len(),
        section::chain(&cut(&points, &triangles, &traces[0])).len()
    );
}

/// The prefilter is a count, not a stopwatch: a short trace over a big sheet
/// tests a small part of the (segment, triangle) pairs, and every triangle that
/// gave a cut was among those tested.
///
/// To make it fail: make the prefilter keep every triangle (`continue` never
/// taken) - `tested` equals `pairs`.
#[test]
fn the_box_prefilter_spares_the_pairs_a_short_trace_cannot_touch() {
    let (points, triangles) = sheet(40); // 3 200 triangles
    let traces = vec![vec![[10.5, 10.5], [14.5, 10.5]], vec![[30.5, 5.5], [30.5, 9.5]]];
    let (many, work) = cut_many_counted(&points, &triangles, &traces);
    assert_eq!(
        work.pairs,
        2 * triangles.len(),
        "one segment each, over every triangle"
    );
    assert!(work.tested > 0, "a prefilter that keeps nothing proves nothing");
    assert!(
        work.tested * 20 < work.pairs,
        "short traces over a 40 × 40 sheet must test a small part of the pairs: {work:?}"
    );
    // Each answer is still `cut`'s own.
    for (at, trace) in traces.iter().enumerate() {
        assert_eq!(
            bits(&many[at]),
            bits(&cut(&points, &triangles, trace)),
            "trace {at}"
        );
        assert!(!many[at].is_empty());
    }
}
