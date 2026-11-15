//! `Path::direction_at`: the unit derivative of the interpolant `Path::at` evaluates (WP04 F2), and
//! `Path::with_vertical`.
//!
//! Built by hand, no file read. The references: RESQML TRG 2.2 §3.8 (the line kinds, *"the depth to
//! Z conversion depends on the vertical CRS direction"*, *"null value: no line"*) and the decision
//! S5 of the specification of the lot (at a knot of a linear line, the interval that ends there).

use energyml_geometry::parametric::{Path, Vertical, VerticalBasis, VerticalReading};

fn close(a: [f64; 3], b: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|axis| (a[axis] - b[axis]).abs() < tolerance)
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

#[test]
fn a_linear_line_at_a_knot_takes_the_interval_that_ends_there() {
    // S5. Break it by taking the interval below at a knot (comparing with `>=`).
    let path = Path::new(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, 100.0], [100.0, 0.0, 100.0]],
        vec![0.0, 100.0, 200.0],
        None,
        Vertical::Down,
    )
    .expect("a linear line");
    assert_eq!(path.direction_at(100.0), Some([0.0, 0.0, 1.0]));
    assert_eq!(path.direction_at(100.0 + 1e-9), Some([1.0, 0.0, 0.0]));
    assert_eq!(path.direction_at(0.0), Some([0.0, 0.0, 1.0]));
    // `at` evaluates the knot in the same interval.
    assert_eq!(path.at(100.0), [0.0, 0.0, 100.0]);
}

#[test]
fn a_vertical_line_points_down_its_depth_axis() {
    // RESQML TRG 2.2 §3.8. Break it by ignoring `vertical`.
    let down = Path::new(0, vec![[275.0, 75.0, 0.0]], vec![0.0], None, Vertical::Down).unwrap();
    let up = Path::new(0, vec![[275.0, 75.0, 0.0]], vec![0.0], None, Vertical::Up).unwrap();
    assert_eq!(down.direction_at(10.0), Some([0.0, 0.0, 1.0]));
    assert_eq!(up.direction_at(10.0), Some([0.0, 0.0, -1.0]));
}

#[test]
fn a_minimum_curvature_direction_is_the_rotated_tangent() {
    // RESQML TRG 2.2 §3.8 (kind 5): the quarter circle. Break it by returning the chord.
    let path = Path::new(
        5,
        vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        vec![0.0, 1.0],
        Some(vec![[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]]),
        Vertical::Down,
    )
    .unwrap();
    let half = std::f64::consts::FRAC_1_SQRT_2;
    assert!(close(path.direction_at(0.5).unwrap(), [-half, half, 0.0], 1e-9));
    assert!(close(path.direction_at(0.0).unwrap(), [0.0, 1.0, 0.0], 1e-9));
    assert!(close(path.direction_at(1.0).unwrap(), [-1.0, 0.0, 0.0], 1e-9));
}

#[test]
fn every_kind_agrees_with_a_centred_difference() {
    // Two independent doors: the analytic derivative, and a difference of `at`. Break it by dropping
    // the `moments` term of the cubic, or the closing term of the arc.
    let control = vec![
        [0.0, 0.0, 0.0],
        [10.0, 0.0, -10.0],
        [30.0, 5.0, -25.0],
        [60.0, 20.0, -30.0],
    ];
    let parameters = vec![0.0, 15.0, 40.0, 75.0];
    let h = 1e-5;
    for kind in [1, 2, 3, 4, 5] {
        let path = Path::new(kind, control.clone(), parameters.clone(), None, Vertical::Up).unwrap();
        for parameter in [7.5, 27.5, 57.5, -5.0, 80.0] {
            let analytic = path.direction_at(parameter).expect("a direction");
            let before = path.at(parameter - h);
            let after = path.at(parameter + h);
            let numeric = unit([after[0] - before[0], after[1] - before[1], after[2] - before[2]]);
            assert!(
                close(analytic, numeric, 1e-6),
                "kind {kind} at {parameter}: {analytic:?} against {numeric:?}"
            );
        }
    }
}

#[test]
fn a_null_line_and_a_nan_parameter_have_no_direction() {
    // RESQML TRG 2.2 §3.8 (*"null value: no line"*). Break it by returning a zero vector.
    let null = Path::new(-1, vec![[0.0, 0.0, 0.0]], vec![0.0], None, Vertical::Down).unwrap();
    assert_eq!(null.direction_at(0.0), None);
    let line = Path::new(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, 10.0]],
        vec![0.0, 10.0],
        None,
        Vertical::Down,
    )
    .unwrap();
    assert_eq!(line.direction_at(f64::NAN), None);
}

#[test]
fn a_degenerate_interval_has_no_direction() {
    // Refuse rather than draw. Break it by returning `Some` of a zero vector.
    let path = Path::new(
        1,
        vec![[5.0, 5.0, 5.0], [5.0, 5.0, 5.0]],
        vec![0.0, 10.0],
        None,
        Vertical::Down,
    )
    .unwrap();
    assert_eq!(path.direction_at(3.0), None);
}

#[test]
fn with_vertical_changes_only_the_depth_reading() {
    // Module doc "Vertical"; S21. Break it by ignoring the argument, or by not copying `basis`.
    let vertical = Path::new(0, vec![[0.0, 0.0, 0.0]], vec![0.0], None, Vertical::Down).unwrap();
    assert_eq!(vertical.at(100.0), [0.0, 0.0, 100.0]);
    let up = vertical.with_vertical(VerticalReading {
        vertical: Vertical::Up,
        basis: VerticalBasis::Crs,
    });
    assert_eq!(up.at(100.0), [0.0, 0.0, -100.0]);
    assert_eq!(up.vertical_basis(), &VerticalBasis::Crs);
    assert_eq!(up.vertical_note(), None);

    let linear = Path::new(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, 10.0]],
        vec![0.0, 10.0],
        None,
        Vertical::Down,
    )
    .unwrap();
    let before = linear.at(5.0);
    let after = linear
        .with_vertical(VerticalReading {
            vertical: Vertical::Up,
            basis: VerticalBasis::NoCrs,
        })
        .at(5.0);
    assert_eq!(before, after, "a linear line stores its z");
}
