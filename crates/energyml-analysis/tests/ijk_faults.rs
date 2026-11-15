//! `energyml_analysis::ijk_faults` without a file: which pillars a fault
//! splits, which side goes down, which cells go - on a 4 × 4 grid small enough
//! to state every answer. The end-to-end confrontation with the reader is
//! `crates/energyml-build/tests/faulted_grid.rs`.

use energyml_analysis::fault_throw::{Profile, Sense};
use energyml_analysis::ijk::{Crossing, Request, Stacked, stack};
use energyml_analysis::ijk_faults::{CutError, Fault, Settings, cut};
use energyml_analysis::surface::Up;

type Point = [f64; 3];

const X0: f64 = 651_000.0;
const Y0: f64 = 6_861_000.0;

/// A flat two-horizon grid of `n × n` cells, 10 m apart, from 0 down to
/// −20 in two layers, in elevation.
fn grid(n: usize) -> (Stacked, Vec<[f64; 2]>) {
    let nodes = (n + 1) * (n + 1);
    let heights = vec![vec![0.0; nodes], vec![-20.0; nodes]];
    let built = stack(&Request {
        heights: &heights,
        counts: [n + 1, n + 1],
        layers: &[2],
        step: 1,
        up: Up::PlusZ,
        crossing: Crossing::Refuse,
    })
    .expect("a grid");
    let mut xy = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            xy.push([X0 + 10.0 * i as f64, Y0 + 10.0 * j as f64]);
        }
    }
    (built, xy)
}

/// A plane striking north through x = X0 + `x` at z = −10, dipping `dip`°
/// east, from +50 to −70, y far past the grid on both sides.
fn plane(x: f64, dip: f64) -> (Vec<Point>, Vec<[u32; 3]>) {
    let at = |z: f64| X0 + x + (-10.0 - z) / dip.to_radians().tan();
    (
        vec![
            [at(50.0), Y0 - 1000.0, 50.0],
            [at(50.0), Y0 + 1000.0, 50.0],
            [at(-70.0), Y0 - 1000.0, -70.0],
            [at(-70.0), Y0 + 1000.0, -70.0],
        ],
        vec![[0, 1, 2], [1, 3, 2]],
    )
}

fn fault<'a>(surface: &'a (Vec<Point>, Vec<[u32; 3]>), dead: usize) -> Fault<'a> {
    Fault {
        name: "F",
        points: &surface.0,
        triangles: &surface.1,
        maximum_throw: 8.0,
        sense: Sense::Down,
        dead_columns: dead,
    }
}

/// A fault between columns 1 and 2 of a 4 × 4 grid: the five pillars at
/// i = 2 are split, the columns east of it (the side it dips to) take the
/// split lines, and those lines sit lower than the pillars' own.
#[test]
fn the_pillars_on_the_fault_are_split_and_the_hanging_wall_goes_down() {
    let (built, xy) = grid(4);
    let surface = plane(20.0, 60.0);
    let faulted = cut(
        &built,
        &xy,
        &[fault(&surface, 0)],
        &Settings {
            profile: Profile::Parabolic,
            reach: 1000.0,
            dead_layers: &[],
        },
    )
    .expect("cut");
    assert_eq!(faulted.split_pillars, vec![2, 7, 12, 17, 22]);
    assert_eq!(faulted.split_columns[0], vec![2]);
    assert_eq!(faulted.split_columns[1], vec![2, 6]);
    assert_eq!(faulted.lines, 25 + 5);
    let hw: Vec<usize> = (0..16).filter(|c| faulted.faults[0].hanging_wall[*c]).collect();
    assert_eq!(hw, vec![2, 3, 6, 7, 10, 11, 14, 15]);
    for (split, &pillar) in faulted.split_pillars.iter().enumerate() {
        for level in 0..3 {
            let own = faulted.z[level * faulted.lines + pillar];
            let other = faulted.z[level * faulted.lines + 25 + split];
            assert!(
                other < own,
                "split {split}, level {level}: {other} is not below {own}"
            );
        }
    }
}

/// Removing one column next to the fault removes the hanging-wall
/// column along it, in the layers asked.
#[test]
fn the_cells_next_to_the_fault_are_removed() {
    let (built, xy) = grid(4);
    let surface = plane(20.0, 60.0);
    let faulted = cut(
        &built,
        &xy,
        &[fault(&surface, 1)],
        &Settings {
            profile: Profile::Parabolic,
            reach: 1000.0,
            dead_layers: &[1],
        },
    )
    .expect("cut");
    assert_eq!(faulted.faults[0].dead_columns, vec![2, 6, 10, 14]);
    assert_eq!(faulted.dead, vec![18, 22, 26, 30]);
    assert_eq!(faulted.undefined_cells(), 4);
}

/// A fault out of the grid's reach, a vertical one, a layer past the grid.
#[test]
fn what_cannot_cut_is_refused_by_name() {
    let (built, xy) = grid(4);
    let settings = Settings {
        profile: Profile::Parabolic,
        reach: 1000.0,
        dead_layers: &[],
    };
    let far = plane(5000.0, 60.0);
    assert!(matches!(
        cut(&built, &xy, &[fault(&far, 0)], &settings),
        Err(CutError::Uncut { .. })
    ));
    let steep = plane(20.0, 90.0);
    assert!(matches!(
        cut(&built, &xy, &[fault(&steep, 0)], &settings),
        Err(CutError::Vertical { .. })
    ));
    let surface = plane(20.0, 60.0);
    let past = Settings {
        dead_layers: &[2],
        ..settings
    };
    assert!(matches!(
        cut(&built, &xy, &[fault(&surface, 1)], &past),
        Err(CutError::DeadLayer { layer: 2, layers: 2 })
    ));
    assert!(matches!(cut(&built, &xy, &[], &settings), Err(CutError::NoFault)));
}

/// A plane like [`plane`], but only from `y_from` to `y_to` (relative to `Y0`):
/// a fault that dies out inside the horizon, which `fault_throw` tears.
fn short_plane(x: f64, dip: f64, y_from: f64, y_to: f64) -> (Vec<Point>, Vec<[u32; 3]>) {
    let at = |z: f64| X0 + x + (-10.0 - z) / dip.to_radians().tan();
    (
        vec![
            [at(50.0), Y0 + y_from, 50.0],
            [at(50.0), Y0 + y_to, 50.0],
            [at(-70.0), Y0 + y_from, -70.0],
            [at(-70.0), Y0 + y_to, -70.0],
        ],
        vec![[0, 1, 2], [1, 3, 2]],
    )
}

/// The cut grid and the torn horizon read one fault the same way
/// (`[analysis D2]`): the same trace reading and the same hanging-wall rule
/// live in `fault_throw`, and the grid says the same side goes down as the
/// horizon does. One fault, dipping east, on a 4 × 4 grid and on a 5 × 5
/// horizon of the same nodes: the columns east of the trace are the grid's
/// hanging wall, the nodes east of it are the ones the horizon lets down.
#[test]
fn the_grid_and_the_torn_horizon_agree_on_the_hanging_wall() {
    use energyml_analysis::fault_throw::{Fault as ThrowFault, Settings as ThrowSettings, TearError, throw};

    let surface = short_plane(17.0, 60.0, 4.0, 36.0);

    // The grid: the fault is read at -10, where it is at x = 17.
    let (built, xy) = grid(4);
    let faulted = cut(
        &built,
        &xy,
        &[Fault {
            name: "F",
            points: &surface.0,
            triangles: &surface.1,
            maximum_throw: 8.0,
            sense: Sense::Down,
            dead_columns: 0,
        }],
        &Settings {
            profile: Profile::Parabolic,
            reach: 1000.0,
            dead_layers: &[],
        },
    )
    .expect("cut");
    let grid_east: Vec<bool> = faulted.faults[0].hanging_wall.clone();

    // The horizon: the same fault meets it at z = 0, at x = 17 - 10 / tan 60.
    let mut cloud = Vec::new();
    for j in 0..5 {
        for i in 0..5 {
            cloud.push([X0 + 10.0 * i as f64, Y0 + 10.0 * j as f64, 0.0]);
        }
    }
    let torn = throw(
        &cloud,
        &[ThrowFault {
            name: "F",
            points: &surface.0,
            triangles: &surface.1,
            maximum_throw: 8.0,
            sense: Sense::Down,
        }],
        &ThrowSettings {
            profile: Profile::Parabolic,
            reach: 1000.0,
            heave: false,
            up: true,
        },
    )
    .expect("thrown");

    // Both put the east of the trace in the hanging wall, and it goes down.
    for j in 0..4 {
        for i in 0..4 {
            assert_eq!(grid_east[j * 4 + i], i >= 2, "grid column ({i}, {j})");
        }
    }
    for j in 1..4 {
        for i in 0..5 {
            let z = torn.points[j * 5 + i][2];
            assert_eq!(z < -1e-9, i >= 2, "horizon node ({i}, {j}) at z = {z}");
            if i < 2 {
                assert_eq!(z, 0.0, "the footwall of ({i}, {j}) does not move");
            }
        }
    }

    // And the refusals are the same ones: a vertical fault is refused by both.
    let steep = short_plane(17.0, 90.0, 4.0, 36.0);
    let thrown_steep = throw(
        &cloud,
        &[ThrowFault {
            name: "V",
            points: &steep.0,
            triangles: &steep.1,
            maximum_throw: 8.0,
            sense: Sense::Down,
        }],
        &ThrowSettings {
            profile: Profile::Parabolic,
            reach: 1000.0,
            heave: false,
            up: true,
        },
    );
    assert!(matches!(thrown_steep, Err(TearError::Vertical { .. })));
    // Break it: in `fault_throw::hanging_wall_side` return `-mean.signum()`: the
    // grid and the horizon then both flip, and this test, which states the east
    // side, fails for both. Give `ijk_faults::cut` its own copy with the opposite
    // sign and only the grid half fails.
}
