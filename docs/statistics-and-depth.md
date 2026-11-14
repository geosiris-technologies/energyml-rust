# Statistics, scales and measured depth

Two small toolkits of `energyml-domain` give every view the same arithmetic:
`stats` describes values, `along_md` moves along a measured depth (MD). Both are
pure Rust with no native dependency, so they build for WebAssembly. The numbers
they share (three median steps, ten median steps, three decades, the 1st and
99th percentiles) are decisions recorded in
[ADR 0027](adr/0027-gaps-seams-scales-and-quantiles.md) and listed as the
`stats` theme of the [house rules](house-rules.md).

## Describing values

`stats::Accumulator` takes values in one pass and can be merged with another:
the count, the finite values, the `NaN`, the infinite values and those equal to
the declared `NullValue` are counted apart, and a missing statistic is `None`,
never zero.

```rust,ignore
use energyml_domain::stats::Accumulator;

let mut acc = Accumulator::with_null_value(Some(-999));
acc.extend(&[1.0, 10.0, f64::NAN, 1000.0]);
assert_eq!(acc.finite(), 3);
assert_eq!(acc.not_a_number(), 1);
let decades = acc.decades(); // Some(3.0): the log scale is proposed
```

`quantiles` are exact up to 10 million values. Beyond, a mergeable sketch
(DDSketch, relative error 1 %) answers, and the result says that it is a sketch
and what its error is.

### Scales

`stats::Scale` is the scale of the **value** (linear or log10, bounds, direction).
It is not `color::Scaling`, which places a value on a colour map. A log scale is
proposed (`propose_log`) when the values span at least 3 decades, and automatic
bounds (`auto_bounds`) are the 1st and 99th percentiles. A scale the file states
or the user chooses wins, and a disagreement with the file is said.

### Describing by steps

`stats::describe` is a door by steps, on the model of the package index: a
`Describer` is created for a plan, then advanced slab by slab with `step`, and
`finish` returns the `Description`. A description that was cancelled is not a
partial one.

## Along the measured depth

`along_md` receives MDs already in one unit and one datum (see
`strata_by_md::comparable`); it never compares references itself.

```rust
use energyml_domain::along_md::{
    GapRule, MdStepper, StepMode, bracket, interpolate_row, Interpolations, Nature, LINEAR,
};

let nodes = [3000.0, 3001.0, 3002.0];
let b = bracket(&nodes, 3001.4, 1e-6).unwrap();
assert_eq!((b.lower, b.upper), (1, 2)); // between nodes 1 and 2, t = 0.4

let stepper = MdStepper::new(nodes.to_vec(), StepMode::Nodes).unwrap();
assert_eq!(stepper.step(3000.6, 1).md, 3001.0); // the node strictly beyond

let rule = GapRule::of_axis(&nodes).unwrap(); // at most 3 median steps
let registry = Interpolations::builtin();
let linear = registry.get(LINEAR).unwrap();
let row = interpolate_row(&[1.0], &[3.0], 0.5, 1.0, &rule, linear, &Nature::default()).unwrap();
assert_eq!(row.values, vec![2.0]);
```

- `bracket` says where an MD falls, with a tolerance for "on the node"; an MD
  outside the support is refused with its bounds, never clamped. `nearest_node`
  breaks a tie towards the shallower node. `union_axis` merges the nodes of
  several supports.
- `MdStepper` steps by notches, from node to node or on a fine grid, snaps onto a
  node, and says when it was clamped at an end of the axis.
- `Interpolations` is a registry (nearest, previous, linear, log_linear,
  angular); the additions of the caller are tried first. A method refuses where it
  has no sense: categories and booleans only take nearest and previous, values
  that hold over intervals only previous, a logarithm needs positive values, two
  opposite angles have no shorter way round. Linear on an angle is accepted with
  the note `AngleTakenAsLinear`.
- `interpolate_row` never bridges a missing value (a `NaN` neighbour gives a
  `NaN` cell, counted in `missing`) and never bridges a gap wider than the
  `GapRule` allows. On a node the value is the node's.
- `JUMP_RATIO` is the companion constant: a jump of the value beyond ten median
  steps of its variation is signalled as a seam, not smoothed.

## Units

Resistivity (`ohm.m` and the others of the schema), electric conductivity
(`S/m`) and angle per length (the dogleg severity, `1/30 dega/m`, `rad/m`...)
convert through the one door, `units::convert`, and can be checked against their
quantity class. `units::resistivity_reciprocal` turns a resistivity into the
conductivity of an isotropic material and back, and says so when it cannot.
