# Around the well

A well is a line, and a lot of what a geoscientist draws sits around it: a ring
of radii at each depth, a matrix of values carried by each node, an envelope, a
curtain, a vertical section. All of it answers the same questions: where is the
hole at this measured depth, which way does it go, where are its top and its
right, and how is it laid flat. This page is the one place of the library
(crate `energyml-geometry`) that answers them. It is pure Rust and builds for
WebAssembly.

Every position, direction, frame and offset on this page is **placed**: in
metres, with z up, in the projected frame of the coordinate system the file
names. Angles are in radians. Measured depths stay in the unit the trajectory
states (`PlacedPath::md_uom`) and are never converted silently.

## A trajectory you can evaluate anywhere

`PlacedPath` is the one door to a trajectory that is placed and evaluable at any
measured depth. It has two sources:

```rust
// A RESQML WellboreTrajectoryRepresentation, placed through the CRS its geometry names.
let path = PlacedPath::of(trajectory.as_reflect(), &store)?;

// A WITSML survey: its stations joined by the minimum-curvature arc, placed by
// where the survey stands (datum and compass axes, in metres).
let path = PlacedPath::from_stations(&stations, &placement)?;

let here = path.at(1250.0);          // point, and where the depth falls
let dir = path.direction_at(1250.0); // unit vector towards increasing MD
let tvd = path.tvd(1250.0);          // below the MD datum, None without a datum
```

`at` says where the depth falls against the depths the path states
(`MdRange::Within`, `BeforeFirst`, `AfterLast`, `NotANumber`): a depth outside
the line gets the extended interval, as the line evaluator extends it, and says
so. Nothing is extrapolated without being flagged.

A trajectory without a coordinate system is refused (`PathRefusal::NoCrs`): which
way is up and where the well stands are not stated. If you know the system, declare
it with `PlacedPath::new(path, crs, md_uom)`; the declaration changes the reading,
never the package. A line read without a system keeps depth downward and says that
it assumed it (`Path::vertical_note`), see [ADR 0041](adr/0041-declared-conventions-and-the-vertical-without-crs.md).

## The direction at a depth

`Path::direction_at` is the derivative of the very interpolant that `Path::at`
evaluates, kind by kind: no second curve, no difference of two evaluations. At a
knot of a linear line the direction is that of the interval that ends there, the
one in which the line evaluates that parameter ([ADR 0031](adr/0031-tangent-at-a-knot.md)).
It is `None` where the derivative vanishes, not a zero vector.

## The frame of the hole

```rust
let frames = frames_along(&path, &[1000.0, 1100.0, 1200.0], &FrameOptions::default())?;
for frame in frames.given() {
    // frame.at, frame.tangent, frame.high_side, frame.right, frame.inclination
}
```

`HoleFrame` is the physical frame of the hole at one depth: the centre, the unit
tangent towards increasing MD, the **high side** (the upward vertical projected
across the hole), the **right** of someone looking down the hole, and the
inclination from vertical down.

Two things matter.

- **The hand of the frame.** The placed frame is not always direct: a system
  written northing then easting is indirect, and a cross product on its coordinates
  points to the other side. The library reads the order of the horizontal axes from
  the file (`Crs::axes`, in RESQML 2.0.1 and in EML 2.3) and gives `right` as the
  physical right in either case. A horizontal hole going north has the east on its
  right in an (E, N, up) frame, and so does the same hole written in (N, E, up).
  If the file states no usable order, a frame is refused (`AxisOrderUnknown`) until
  you declare the hand in `FrameOptions`; a declaration never overrides the file
  ([ADR 0032](adr/0032-hole-frame-in-a-direct-placed-frame.md)).
- **A vertical hole has no high side.** Under 1 degree of inclination (or above 179)
  the frame is refused at that node by name (`HighSideUndefined`). `Frames` keeps one
  outcome per depth asked, so a refused node is a hole you can count, not the end of
  the well. The threshold is the constant `MIN_INCLINATION_DEGREES`.

`north_in_plane` gives grid north seen in the plane across the hole, as
(right, high side), when an angle is counted from north. `offset_point` gives the
pose in space of a point moved along the high side. This frame is not the
transported frame of a tube (`tube::sweep`: minimal rotation, arbitrary origin);
the two stay, under two names.

## Angles around the hole: a declared convention

Files give radii and angles without saying where angle zero is or which way angles
grow. The library does not guess. An `AngleConvention` is a parameter you declare
(origin: high side, low side, right, left or north; sense: clockwise or
anticlockwise; viewed from uphole or downhole; and an offset), and without it the
layer is refused (`ConventionUndeclared`).

```rust
let convention = AngleConvention {
    origin: AngleOrigin::HighSide,
    sense: AngleSense::Clockwise,
    viewed_from: ViewedFrom::LookingDownhole,
    offset: 0.0,
};
let ring = polar_points(&radii, &angles, Some(&convention), north)?;
// ring.points: one (right, high side) per radius, None for a counted hole
```

The canonical plane is the one an observer standing uphole and looking down the hole
sees: the right on the right, the high side up, a clockwise turn going from the top
to the right. A missing radius is a hole that is counted (`holes`), never filled; a
negative radius is shown as a hole and counted apart (`negative`). `sector_bounds`
gives the bounds of each sector, halfway to its neighbours, and `to_space` carries a
point of the plane into space by the frame.

## A matrix carried by each node

Some tools carry a two-dimensional matrix at each node (an image across the hole:
81 by 81 values). Which dimension runs along the high side, which along the right,
with which sign and which offsets in metres is not in the file either, so a
`TransverseGrid` is declared too: one `GridAxis` for the slow dimension (the first
count, TRG 2.2 §7.1) and one for the fast.

```rust
let grid = TransverseGrid::new(slow, fast)?;
grid.checked(&shape)?;                    // the property's per-node shape must be [m, n]
let points = grid.slice(&frame);          // m x n points of one node
let folds = grid.check_folds(&frames);    // consecutive slices that cross
```

Values of the axes come from the registry of axis sources (see
[The shape of a property](property-shape.md)); this module only places them.

## Envelope and ribbon

`envelope` builds a mesh whose section varies along the well: radii per sector and
per node, with the angles taken per node, shared, or evenly spread (`Angles`; a
choice of the caller, never a default). The triangles face outwards whatever the
sense of the angles. `ribbon` lays one column of a transverse cube along the well
(`RibbonPlane::FixedFast` or `FixedSlow`). Both give an `EnvelopeMesh`, which says
for each point the index of its value in the property's own array, and counts
`holes`: a quad with a corner missing is not drawn and never bridged.

## Laying a well flat

`WellProjections` is a registry (the caller's additions are tried first) of ways to
lay a well flat. A point `(md, dn)` of the frame of the well, `dn` metres along the
high side, becomes a 2D point:

| id | x | y | x is a length |
|---|---|---|---|
| `md_bar` | 0 | `md` | no |
| `md_tvd` | `md` | TVD | no |
| `developed` | plan length up to `md` (plus `dn cos I`) | TVD | yes |
| `vertical_section` | distance along the section azimuth | TVD | yes |
| `plan` | east | north | yes |

The TVD is counted **below the MD datum**, the point where the measured depth is 0,
not from the zero of the vertical system; a line that does not reach its datum is
refused (`NoDatum`) rather than counted from its first node
([ADR 0033](adr/0033-tvd-below-the-md-datum.md)). The vertical coordinate is the TVD
positive downward; the drawing turns it over.

```rust
let axis = WellAxis::new(&path, [0.0, 3000.0], 10.0)?;   // sampled once
let projections = WellProjections::standard();
let md_tvd = projections.get("md_tvd").unwrap();
let line = locator_line(&axis, md_tvd, 0.5)?;             // fine enough for 0.5 m
let mark = locator_mark(&axis, md_tvd, 1250.0)?;          // where one MD falls
```

`locator_line` and `locator_mark` serve the small map of a well (the position of a
depth, the direction of the tick). A mark is always `PlacedPath::at(md)` projected,
never a length measured on the flat line: on the plan of a J-shaped well the line
is shorter than the measured depth. `locator_md_at` goes back from a point to an MD
and `locator_spans` cuts the line into the pieces inside given MD spans.

## The rules, and how to read them

The four readings that are the library's own choice are in the registry of house
rules, theme *well*: the direction at a knot, the minimum inclination of a high side,
the TVD below the MD datum, and the conventions declared by the caller.

```sh
energyml rules --house --theme well
```

They are declared rules: the code applies them, and a breach is a named refusal,
never a drawing.
