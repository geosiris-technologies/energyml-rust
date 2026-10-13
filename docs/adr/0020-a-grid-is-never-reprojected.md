# 0020 - A grid is never reprojected

Status: accepted

## Context

A structured grid (a 2D grid of heights, an IJK grid) is not a bag of points. Its
nodes sit on a lattice written in the numbers of a frame: counts, steps, an
origin, an order (fastest axis first). Reprojecting its nodes to another frame
would give points that no longer lie on a lattice in either frame, and the grid
could not be written back. Worse, a lattice read in one frame and filled with
values read in another puts every node somewhere else, with no error.

## Decision

A grid keeps the frame it was written in. No operation reprojects a grid; when two
inputs name two frames and one of them is a grid, the operation **refuses by
name**, with both frames in the message, and says what to do instead:

- interpolation onto a grid whose frame is not the samples' is refused ("a grid is
  never reprojected: interpolate onto a grid of the samples' frame, or bring the
  samples into the grid's frame first");
- the frame written for a result on a grid must be the grid's own, so a `crs`
  naming another frame is refused;
- the lattice must be the house one (along +x then +y of its frame), or it is
  refused as turned or reversed.

Things that are not grids (points, polylines, triangulated surfaces) can be moved
between frames with `change-frame`, which keeps their nodes and topology.

## Consequences

- Nothing is resampled without being stated: putting values on another lattice is
  a different operation, interpolation onto a new grid, and it is named as such.
- A user with samples in one frame and a grid in another must bring one to the
  other explicitly.
- The rule is enforced by refusals in `interpolate`, and stated in `grid-properties`,
  `ijk-grid-from-horizons` and the horizon operations; a test checks that the
  `interpolate` message says "never reprojected".

TODO(verify): the list of operations that state the rule, against the final public tree.
