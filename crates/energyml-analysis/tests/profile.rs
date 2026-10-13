//! Wells projected on a long profile.
//!
//! Every geometry is fabricated here and every expected value is worked out by
//! hand beside it. The band is judged by a point-in-polygon test **written in
//! this file** (winding number), independent of the module.

use energyml_analysis::profile::{Corridor, Head, Kept, Profile, ProfileError, Reason, Side, project};

const STEP: f64 = 1.0_f64.to_radians();

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn head(id: &str, x: f64, y: f64) -> Head {
    Head::new(id, [x, y])
}

fn kept<'a>(p: &'a Profile, id: &str) -> &'a Kept {
    p.kept
        .iter()
        .find(|k| k.id == id)
        .unwrap_or_else(|| panic!("{id} should be kept: {p:?}"))
}

fn reason(p: &Profile, id: &str) -> Option<Reason> {
    p.rejected.iter().find(|r| r.id == id).map(|r| r.reason)
}

/// Nonzero winding number of `p` in `poly` (closed implicitly) — the rule the
/// band is documented to be filled with.
fn winding(poly: &[[f64; 2]], p: [f64; 2]) -> i32 {
    let mut w = 0;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let side = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
        if a[1] <= p[1] {
            if b[1] > p[1] && side > 0.0 {
                w += 1;
            }
        } else if b[1] <= p[1] && side < 0.0 {
            w -= 1;
        }
    }
    w
}

fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    winding(poly, p) != 0
}

/// Distance from `p` to the polyline, by brute force over every segment.
fn brute_distance(track: &[[f64; 2]], p: [f64; 2]) -> f64 {
    track
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let ab = [b[0] - a[0], b[1] - a[1]];
            let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / (ab[0] * ab[0] + ab[1] * ab[1]))
                .clamp(0.0, 1.0);
            (p[0] - a[0] - t * ab[0]).hypot(p[1] - a[1] - t * ab[1])
        })
        .fold(f64::INFINITY, f64::min)
}

/// A straight track east, left 10 and right 5. A head at 8 to the north is on
/// the left, at 4 to the south on the right; 6 to the south is past the right
/// width, and exactly 10 to the north is past the left width (`<`, not `<=`).
/// To break: flip the sign of the cross product, write `<=`, or file the
/// segments in their own cells only (the head at y = -4 lives in another row) —
/// each one turns a line red.
#[test]
fn a_straight_track_keeps_by_the_width_of_each_side() {
    let track = [[0.0, 0.0], [100.0, 0.0]];
    let heads = [
        head("north", 50.0, 8.0),
        head("south", 50.0, -4.0),
        head("far-south", 50.0, -6.0),
        head("limit", 50.0, 10.0),
        head("remote", 50.0, 500.0),
    ];
    let c = Corridor::new(&track, 10.0, 5.0).unwrap();
    let p = c.project(&heads);
    let n = kept(&p, "north");
    assert_eq!((n.side, n.foot), (Side::Left, [50.0, 0.0]));
    assert!(close(n.s, 50.0) && close(n.d, 8.0));
    let s = kept(&p, "south");
    assert_eq!(s.side, Side::Right);
    assert!(close(s.s, 50.0) && close(s.d, 4.0));
    assert_eq!(reason(&p, "far-south"), Some(Reason::TooFarOnItsSide));
    assert_eq!(
        reason(&p, "limit"),
        Some(Reason::TooFarOnItsSide),
        "exactly on the limit is not under it"
    );
    assert!(
        p.kept.iter().all(|k| k.id != "remote") && reason(&p, "remote").is_none(),
        "far wells are not listed"
    );
    assert!(close(c.length(), 100.0));
}

/// Beyond an end: ignored and counted. West of the start the head
/// (-3, 2) is at 3.6 from the end, under the left width 10; (104, -1) is
/// hypot(4, 1) = 4.12 from the other end, under the right width 5. (-3, -6) is
/// 6.7 from the start on the right side (width 5): out of reach, not listed.
/// To break: let a head with `t < 0` fall back onto the end vertex as a kept
/// candidate (the first two would enter `kept`), or stop counting.
#[test]
fn a_head_beyond_an_end_is_ignored_and_counted() {
    let track = [[0.0, 0.0], [100.0, 0.0]];
    let heads = [
        head("before", -3.0, 2.0),
        head("after", 104.0, -1.0),
        head("lost", -3.0, -6.0),
        head("on", 10.0, 1.0),
    ];
    let p = Corridor::new(&track, 10.0, 5.0).unwrap().project(&heads);
    assert_eq!(p.kept.len(), 1);
    assert_eq!(p.kept[0].id, "on");
    assert_eq!(reason(&p, "before"), Some(Reason::BeyondAnEnd));
    assert_eq!(reason(&p, "after"), Some(Reason::BeyondAnEnd));
    assert_eq!(p.beyond_an_end(), 2);
    assert!(reason(&p, "lost").is_none());
}

/// An L: east 100 then north 100, left 10, right 20 (a left turn: the left is
/// the inside, the right the outside).
///
/// - (110, -10) is in the outer wedge (x > 100 and y < 0): projected on the
///   vertex (100, 0), `s` = 100, `d` = 10 sqrt 2 = 14.142, on the RIGHT (the
///   bisector direction is (1, 1), cross((1,1), (10,-10)) = -20 < 0) and kept
///   under the right width 20 although over the left width 10;
/// - (120, -1) in the same wedge is at 20.02, over the reach 20: not kept, not listed;
/// - (95, 3) inside the turn projects on both segments (3 and 5 from them):
///   the first, `s` = 95, `d` = 3; (97, 8) is nearer the second (3 against 8):
///   `s` = 100 + 8 = 108, `d` = 3;
/// - (104, 108) is beyond the end of the last segment, 8.9 from it on the
///   right (cross((0,1), (4,8)) = -4) under 20: counted, not drawn.
///
/// To break: remove the wedge (the first one disappears), take the far
/// projection, or read the vertex's side with the left width.
#[test]
fn an_l_has_its_outer_wedge_on_the_vertex_and_its_inner_turn_on_the_nearest_segment() {
    let track = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]];
    let heads = [
        head("outer", 110.0, -10.0),
        head("outer-far", 120.0, -1.0),
        head("inner-a", 95.0, 3.0),
        head("inner-b", 97.0, 8.0),
        head("past-end", 104.0, 108.0),
    ];
    let p = Corridor::new(&track, 10.0, 20.0).unwrap().project(&heads);
    let o = kept(&p, "outer");
    assert_eq!((o.side, o.foot), (Side::Right, [100.0, 0.0]));
    assert!(close(o.s, 100.0) && close(o.d, 200.0_f64.sqrt()), "{o:?}");
    assert!(
        p.kept.iter().all(|k| k.id != "outer-far") && reason(&p, "outer-far").is_none(),
        "over the reach: not listed"
    );
    let a = kept(&p, "inner-a");
    assert!(
        close(a.s, 95.0) && close(a.d, 3.0) && a.side == Side::Left,
        "{a:?}"
    );
    assert_eq!(a.foot, [95.0, 0.0]);
    let b = kept(&p, "inner-b");
    assert!(
        close(b.s, 108.0) && close(b.d, 3.0) && b.side == Side::Left,
        "{b:?}"
    );
    assert_eq!(b.foot, [100.0, 8.0]);
    assert_eq!(reason(&p, "past-end"), Some(Reason::BeyondAnEnd));
    assert_eq!(p.beyond_an_end(), 1);
}

/// The nearest-vertex trap. The track runs 1000 east, then back west to (520, 100) and
/// north to (520, 500); the head (500, 30) is halfway along the first segment,
/// 30 from it, and 500 from both its ends. The nearest VERTEX is (520, 100), at
/// hypot(20, 70) = 72.8 — over the width 50 — and none of its two segments is
/// the nearest one: "the nearest vertex first" drops the well without a word.
/// The right answer: kept, `d` = 30, `s` = 500.
/// To break: restrict the candidates to the segments of the nearest vertex.
#[test]
fn a_nearer_vertex_does_not_hide_the_nearest_segment() {
    let track = [[0.0, 0.0], [1000.0, 0.0], [520.0, 100.0], [520.0, 500.0]];
    let nearest_vertex = track
        .iter()
        .map(|v| (v[0] - 500.0_f64).hypot(v[1] - 30.0))
        .fold(f64::INFINITY, f64::min);
    assert!(
        nearest_vertex > 50.0,
        "the fixture must trap the nearest-vertex shortcut"
    );
    let p = Corridor::new(&track, 50.0, 50.0)
        .unwrap()
        .project(&[head("w", 500.0, 30.0)]);
    let k = kept(&p, "w");
    assert!(
        close(k.d, 30.0) && close(k.s, 500.0) && k.side == Side::Left,
        "{k:?}"
    );
}

/// The band keeps exactly what the criterion keeps, on lattices of heads over
/// four fabricated tracks (straight, L, an S with a short middle segment, a
/// hairpin), with two pairs of widths. Judged by the winding test above:
///
/// - inside the polygon means kept (the arcs are inscribed: nothing dropped is
///   ever inside);
/// - kept with `d` more than 0.05 under its side's width means inside (the
///   sagitta at 1 degree and a width of 25 is 0.002);
/// - the distance of a kept head is never under the brute-force distance to
///   the polyline, and on the tracks without self-approach it equals it.
///
/// The counts are asserted so the loop cannot pass on empty sets.
/// To break: leave the outer turns as chords (the corner heads fall outside),
/// cut the inner turns at the vertex instead of at the crossing, or give the
/// left edge the right width.
#[test]
fn the_band_contains_every_kept_head_and_no_set_aside_one() {
    let tracks: [(&str, Vec<[f64; 2]>, bool); 4] = [
        ("straight", vec![[0.0, 0.0], [100.0, 0.0]], true),
        ("L", vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]], true),
        (
            "S",
            vec![[0.0, 0.0], [100.0, 0.0], [100.0, 3.0], [200.0, 3.0]],
            true,
        ),
        (
            "hairpin",
            vec![[0.0, 0.0], [100.0, 0.0], [100.0, 30.0], [0.0, 30.0]],
            false,
        ),
    ];
    for (left, right) in [(20.0, 20.0), (10.0, 25.0)] {
        for (name, track, simple) in &tracks {
            let c = Corridor::new(track, left, right).unwrap();
            let band = c.band(STEP).unwrap();
            let (mut n_in, mut n_out) = (0, 0);
            let (mut x, mut heads) = (-45.3, Vec::new());
            while x < 250.0 {
                let mut y = -46.1;
                while y < 150.0 {
                    heads.push(head(&format!("{x}/{y}"), x, y));
                    y += 3.7;
                }
                x += 3.7;
            }
            let profile = c.project(&heads);
            for h in &heads {
                let is_inside = inside(&band, h.xy);
                match profile.kept.iter().find(|k| k.id == h.id) {
                    Some(k) => {
                        let w = if k.side == Side::Left { left } else { right };
                        let brute = brute_distance(track, h.xy);
                        assert!(k.d >= brute - 1e-9, "{name}: {h:?} {k:?} brute {brute}");
                        if *simple {
                            assert!(close(k.d, brute), "{name}: {h:?} {k:?} brute {brute}");
                        }
                        if k.d < w - 0.05 {
                            assert!(
                                is_inside,
                                "{name} {left}/{right}: kept {h:?} {k:?} outside the band"
                            );
                        }
                        if is_inside {
                            n_in += 1;
                        }
                    }
                    None => {
                        assert!(
                            !is_inside,
                            "{name} {left}/{right}: {h:?} is inside the band but not kept"
                        );
                        n_out += 1;
                    }
                }
            }
            assert!(
                n_in > 20 && n_out > 20,
                "{name}: {n_in} in, {n_out} out — a loop over nothing proves nothing"
            );
        }
    }
}

/// The band of the L (left 10, right 20, ring counter-clockwise, ends
/// straight): its edge at the inner corner passes through the crossing of the
/// two left offsets, (90, 10); the head (112, -12) of the outer wedge — 16.97
/// from the vertex, under 20 — is inside it, which a chord from (100, -20) to
/// (120, 0) would not give; and the straight ends do not reach past the track:
/// (-1, 0) is outside. The area, by hand: two strips of 100 x 30 overlapping on
/// the inner 10 x 10, plus the quarter disc of radius 20 at the outer corner:
/// 6000 - 100 + 100 pi.
/// To break: replace the arc by its chord, or extend the ends.
#[test]
fn the_band_of_an_l_is_rounded_outside_cut_inside_and_straight_at_its_ends() {
    let track = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]];
    let c = Corridor::new(&track, 10.0, 20.0).unwrap();
    let band = c.band(STEP).unwrap();
    let area: f64 = (0..band.len())
        .map(|i| {
            let (a, b) = (band[i], band[(i + 1) % band.len()]);
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        / 2.0;
    assert!(area > 0.0, "counter-clockwise");
    assert!(
        band.iter()
            .any(|p| (p[0] - 90.0).abs() < 1e-9 && (p[1] - 10.0).abs() < 1e-9),
        "the inner crossing"
    );
    assert!(inside(&band, [112.0, -12.0]));
    assert!(!inside(&band, [-1.0, 0.0]) && !inside(&band, [101.0, 101.0]));
    assert!(inside(&band, [0.5, 0.0]) && inside(&band, [0.5, -19.0]) && inside(&band, [0.5, 9.0]));
    assert!(!inside(&band, [0.5, 10.5]) && !inside(&band, [0.5, -20.5]));
    let exact = 5900.0 + 100.0 * std::f64::consts::PI;
    assert!((area - exact).abs() < 1.0, "area {area}, expected {exact}");
}

/// The refusals of a degenerate track, by name: one vertex, none, two
/// coincident vertices (the index says which segment), a vertex that is `NaN`;
/// and the widths and the step.
/// To break: let a zero-length segment through — its direction is `0/0`.
#[test]
fn a_degenerate_track_is_refused_by_its_name() {
    assert_eq!(
        project(&[[0.0, 0.0]], 5.0, 5.0, &[], STEP).unwrap_err(),
        ProfileError::TooFewVertices { found: 1 }
    );
    assert_eq!(
        project(&[], 5.0, 5.0, &[], STEP).unwrap_err(),
        ProfileError::TooFewVertices { found: 0 }
    );
    assert_eq!(
        project(&[[0.0, 0.0], [0.0, 0.0]], 5.0, 5.0, &[], STEP).unwrap_err(),
        ProfileError::ZeroLengthSegment { index: 0 }
    );
    assert_eq!(
        project(
            &[[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [2.0, 0.0]],
            5.0,
            5.0,
            &[],
            STEP
        )
        .unwrap_err(),
        ProfileError::ZeroLengthSegment { index: 1 }
    );
    assert_eq!(
        project(&[[0.0, 0.0], [f64::NAN, 0.0]], 5.0, 5.0, &[], STEP).unwrap_err(),
        ProfileError::NonFiniteVertex { index: 1 }
    );
    let line = [[0.0, 0.0], [10.0, 0.0]];
    assert!(matches!(
        project(&line, -1.0, 5.0, &[], STEP),
        Err(ProfileError::BadWidth { side: Side::Left, .. })
    ));
    assert!(matches!(
        project(&line, 5.0, f64::NAN, &[], STEP),
        Err(ProfileError::BadWidth {
            side: Side::Right,
            ..
        })
    ));
    assert_eq!(
        project(&line, 0.0, 0.0, &[], STEP).unwrap_err(),
        ProfileError::NoWidth
    );
    assert_eq!(
        project(&line, 5.0, 5.0, &[], 0.0).unwrap_err(),
        ProfileError::BadArcStep(0.0)
    );
}

/// A head without a position is set aside with its own reason, never placed at
/// the origin; a zero width on one side keeps nothing on that side, and a head
/// on the axis then counts on the side that has a width.
/// To break: let `NaN` through to the distance (it compares false against any
/// width and the well would vanish without a trace).
#[test]
fn a_head_without_a_position_is_set_aside_and_a_zero_side_keeps_nothing() {
    let line = [[0.0, 0.0], [10.0, 0.0]];
    let heads = [
        head("nan", f64::NAN, 1.0),
        head("left", 5.0, 1.0),
        head("right", 5.0, -1.0),
        head("axis", 5.0, 0.0),
    ];
    let section = project(&line, 0.0, 3.0, &heads, STEP).unwrap();
    let p = &section.profile;
    assert_eq!(reason(p, "nan"), Some(Reason::NoPosition));
    assert_eq!(reason(p, "left"), Some(Reason::TooFarOnItsSide));
    assert_eq!(kept(p, "right").side, Side::Right);
    let a = kept(p, "axis");
    assert!(a.side == Side::Right && close(a.d, 0.0), "{a:?}");
    assert!(close(section.length, 10.0));
}

/// `Kept::direction` is the direction of the track where the well was kept —
/// the segment the projection retained, and at the outer wedge of a turn the
/// **mean** of the two segments' directions, normalised. On the L of the test above: east then north.
///
/// - halfway along the first segment: (1, 0);
/// - inside the turn, projected on the second segment: (0, 1) — not the
///   first's, though the head is nearer the vertex than the middle of either;
/// - the outer wedge, on the vertex: (1, 1) / sqrt 2, unit length.
///
/// To break: give the corner the first segment's direction (the corner test
/// turns red), or the unnormalised sum of both (the norm is no longer 1).
#[test]
fn a_kept_well_carries_the_direction_of_the_track_where_it_was_kept() {
    let track = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]];
    let heads = [
        head("middle", 50.0, 4.0),
        head("inner-b", 97.0, 8.0),
        head("outer", 110.0, -10.0),
    ];
    let p = Corridor::new(&track, 10.0, 20.0).unwrap().project(&heads);
    let unit = |k: &Kept| (k.direction[0].hypot(k.direction[1]) - 1.0).abs() < 1e-12;

    let m = kept(&p, "middle");
    assert!(close(m.direction[0], 1.0) && close(m.direction[1], 0.0), "{m:?}");
    let b = kept(&p, "inner-b");
    assert_eq!(b.foot, [100.0, 8.0], "the fixture is on the second segment");
    assert!(close(b.direction[0], 0.0) && close(b.direction[1], 1.0), "{b:?}");
    let o = kept(&p, "outer");
    assert_eq!(o.foot, [100.0, 0.0], "the fixture is on the vertex");
    let half = std::f64::consts::FRAC_1_SQRT_2;
    assert!(
        close(o.direction[0], half) && close(o.direction[1], half),
        "{o:?}"
    );
    assert!(p.kept.iter().all(unit));
}
