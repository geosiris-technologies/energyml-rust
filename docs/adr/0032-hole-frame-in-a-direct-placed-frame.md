# 0032 - The frame of a hole is the physical one, whatever hand the placed frame has

Status: accepted

## Context

Around a well, a view needs more than the direction: where is the top of the
hole, and where is its right. Rings, matrices carried by each node, envelopes
and a curtain all hang on that pair. It is computed from a tangent and the
upward vertical, and its sign depends on a cross product.

A cross product taken on coordinates points to the physical right only when the
frame of those coordinates is direct. The placed frame (first projected axis,
second projected axis, elevation) is not always direct: a system whose axes are
written northing then easting is indirect, and a cross product on its
coordinates points to the other side. The coordinate system local to a tool
(east, north, depth) is indirect as well. Computing the frame in the local
coordinates, or in an unchecked placed frame, gives a right that is wrong and
looks right.

## Decision

- **The frame is computed in the placed frame** (metres, z up), where the vertical
  has already been turned by the coordinate system; nothing is computed in the
  local coordinates.
- **The hand is read from the file**, once, in `Crs::axes`
  (`AxisOrder`, `AxisReading`, `Handedness`, `CompassAxes`): the order of the
  horizontal axes of the projected system in RESQML 2.0.1, `AxisOrder` and
  `HorizontalAxes` in EML 2.3. Directions go through `Crs::direction_to_projected`,
  the door for vectors as `to_projected` is the door for points.
- **`right` is always the physical right** of someone looking down the hole,
  written in the placed coordinates: `tangent x high_side` in a direct frame, its
  opposite in an indirect one. Two writings of the same well, one in (E, N, up)
  and one in (N, E, up), give the same physical right.
- **The high side** is the upward vertical projected across the hole. Below 1
  degree of inclination (and above 179) it is not defined, and the frame is
  refused at that node only (`HighSideUndefined`), never turned to north in
  silence. The threshold is one named constant, `MIN_INCLINATION_DEGREES`.
- **A hand the file does not give is declared by the caller** (ADR 0041). A
  declaration never overrides what the file states: a disagreement is refused
  (`DeclaredAgainstFile`). Two statements of the same frame that disagree make
  the reading `Contradictory`, and a frame that needs the hand is refused by
  name (`AxisOrderUnknown`). Reading the axes never makes the placement of a
  point fail: only what needs the hand refuses.
- The frame of a hole is distinct from the transported frame of a tube
  (minimal rotation, arbitrary origin): both stay, under two names.

## Consequences

- The right of a ring or of a matrix is the same in a placed frame of either
  hand; a test writes one well both ways and compares.
- A package whose system states nothing about its axes gets no frame until the
  caller declares a hand; it is a refusal with its reason, not a drawing.
- `Crs` gains a field: a literal `Crs { .. }` in a test has to say
  `axes: AxisReading::NotStated`. A published release carrying it is a breaking
  change of the public type.
- Which of the projected `AxisOrder` and the local `HorizontalAxes` the
  specification means to govern in EML 2.3 is not stated; the projected one is
  taken first, because the placed coordinates are those of the projected system.
