# 0033 - A true vertical depth along a well is counted below the MD datum

Status: accepted

## Context

A view that lays a well flat (a measured depth against a true vertical depth, a
developed length, a vertical section, a plan) needs a vertical coordinate.
"True vertical depth" has two origins in practice: the zero of the vertical
system (a depth below the sea level or the ground, read straight from `z`), or
the point where the measured depth is 0, the datum of the MD (the rotary table,
the kelly bushing). A well drawn from the first reads shifted by the height of
its table against a log drawn from the second, and the two columns of a sheet
then disagree by a constant nobody can see.

The same question comes up in several views. Two of them had their own
projection for the same formula (a measured depth against a TVD, a section along
an azimuth), under two names.

## Decision

- **The TVD of a projection or of a curtain is counted downward from the MD datum**:
  `PlacedPath::datum().z - z`. It is registered as `house/well/tvd-below-md-datum`.
- **A line that does not reach MD 0 has no datum here**, and a projection that asks
  for a TVD is refused by name (`NoDatum`). Counting from the first node would
  be an extrapolation made silently.
- **One formula per projection, one registry**: `WellProjections`, a trait and a
  registry in which the caller's additions are tried first, as for the other
  registries of the library. The standard entries are `md_bar`, `md_tvd`,
  `developed` and `plan`; `vertical_section` takes an azimuth and an origin and
  is built by the caller. Two earlier projections that were the same formula
  are merged: the unfolding by measured depth and the MD/TVD mode are one entry,
  the unfolding by azimuth and the vertical section are one entry
  (`VerticalSection::abscissa` is its only formula).
- **The position of a mark is always `PlacedPath::at(md)` projected**, never a
  length measured on the flat line: on the plan of a J-shaped well the flat
  line is shorter than the MD.
- **The well is sampled once** (`WellAxis`: the ends of the range, the knots
  inside it and a step) and every projection reads that sampling.

## Consequences

- A measured depth and a true vertical depth start together at the datum, as a
  driller reads them, whatever vertical system the package uses.
- A trajectory that starts below MD 0 can still be drawn by measured depth and in
  plan; only the projections that need a TVD refuse.
- Adding a projection is adding an entry, not a branch.
- The vertical axis of these projections is the TVD positive downward; the drawing
  turns it over.
