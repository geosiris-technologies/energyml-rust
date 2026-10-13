//! `energyml_analysis::ijk_faults` without a file: which pillars a fault
//! splits, which side goes down, which cells go — on a 4 × 4 grid small enough
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
    let built = stack(&Request { heights: &heights, counts: [n + 1, n + 1], layers: &[2], step: 1, up: Up::PlusZ, crossing: Crossing::Refuse })
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
        vec![[at(50.0), Y0 - 1000.0, 50.0], [at(50.0), Y0 + 1000.0, 50.0], [at(-70.0), Y0 - 1000.0, -70.0], [at(-70.0), Y0 + 1000.0, -70.0]],
        vec![[0, 1, 2], [1, 3, 2]],
    )
}

fn fault<'a>(surface: &'a (Vec<Point>, Vec<[u32; 3]>), dead: usize) -> Fault<'a> {
    Fault { name: "F", points: &surface.0, triangles: &surface.1, maximum_throw: 8.0, sense: Sense::Down, dead_columns: dead }
}

/// A fault between columns 1 and 2 of a 4 × 4 grid: the five pillars at
/// i = 2 are split, the columns east of it (the side it dips to) take the
/// split lines, and those lines sit lower than the pillars' own.
#[test]
fn the_pillars_on_the_fault_are_split_and_the_hanging_wall_goes_down() {
    let (built, xy) = grid(4);
    let surface = plane(20.0, 60.0);
    let faulted =
        cut(&built, &xy, &[fault(&surface, 0)], &Settings { profile: Profile::Parabolic, reach: 1000.0, dead_layers: &[] }).expect("cut");
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
            assert!(other < own, "split {split}, level {level}: {other} is not below {own}");
        }
    }
}

/// Removing one column next to the fault removes the hanging-wall
/// column along it, in the layers asked.
#[test]
fn the_cells_next_to_the_fault_are_removed() {
    let (built, xy) = grid(4);
    let surface = plane(20.0, 60.0);
    let faulted =
        cut(&built, &xy, &[fault(&surface, 1)], &Settings { profile: Profile::Parabolic, reach: 1000.0, dead_layers: &[1] }).expect("cut");
    assert_eq!(faulted.faults[0].dead_columns, vec![2, 6, 10, 14]);
    assert_eq!(faulted.dead, vec![18, 22, 26, 30]);
    assert_eq!(faulted.undefined_cells(), 4);
}

/// A fault out of the grid's reach, a vertical one, a layer past the grid.
#[test]
fn what_cannot_cut_is_refused_by_name() {
    let (built, xy) = grid(4);
    let settings = Settings { profile: Profile::Parabolic, reach: 1000.0, dead_layers: &[] };
    let far = plane(5000.0, 60.0);
    assert!(matches!(cut(&built, &xy, &[fault(&far, 0)], &settings), Err(CutError::Uncut { .. })));
    let steep = plane(20.0, 90.0);
    assert!(matches!(cut(&built, &xy, &[fault(&steep, 0)], &settings), Err(CutError::Vertical { .. })));
    let surface = plane(20.0, 60.0);
    let past = Settings { dead_layers: &[2], ..settings };
    assert!(matches!(cut(&built, &xy, &[fault(&surface, 1)], &past), Err(CutError::DeadLayer { layer: 2, layers: 2 })));
    assert!(matches!(cut(&built, &xy, &[], &settings), Err(CutError::NoFault)));
}
