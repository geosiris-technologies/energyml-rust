# 0026 - A head portion says whether the file describes it, and is in the line's frame

Status: accepted

## Context

A well frame starts at its first node, below the measured depth (MD) 0 of the
trajectory. The stretch between MD 0, the datum, and that first node is the head
portion, and a view draws it. Two defects hid in its calculation:

- **The result did not say what it was.** The doc stated that no object describes the
  interval, which is false when the trajectory's own line covers MD 0. Whether the
  portion is the file's line or a supposition decides how it is drawn.
- **Two frames were mixed.** When the trajectory has a `Geometry` whose `MdMin` is
  above 0, the portion was drawn from the datum, already placed (projected, elevation
  upwards), while the nodes of the frame are on the line, in its own local CRS as
  stored. A caller places both by the same placement, so the datum was placed twice,
  far from the well and upside down, and the portion did not join the frame.

The datum is at MD 0: "for purposes of the trajectory the MdDatum within the
MdInterval is mandatory" (RESQML TRG 2.2 section 12.17, p. 314). The line is the
trajectory and its parameter is the MD. Outside its nodes it extends its first or
last interval (RESQML TRG 2.2 section 3.8).

## Decision

- **`head_portion` returns `HeadPortion::Described(points)` or `Assumed(points)`.**
  `Described`: the trajectory's line covers MD 0 to the first node (`MdMin` at or
  below 0), the points are samples of that line, and the datum is not even read.
  `Assumed`: the portion contains something the file does not state.
- **One frame, the line's own, as stored.** Every point of the portion is expressed
  there, and the caller places them with the same call as the nodes of the frame. The
  only point that comes from elsewhere, the datum, is brought into that frame by the
  exact inverse of the CRS's placement (`Crs::from_projected`), never placed apart.
- **The straight chord from the datum to the start of the line is assumed.** When
  `MdMin` is above 0 the file says where MD 0 is and where the line starts, not the
  path between: the portion is the datum, the chord, then the line itself, drawn as an
  assumption. A chord is the one shape that invents no curvature. Without a
  `Geometry`, the vertical segment between the datum's two anchors is assumed the
  same way.
- **A datum that cannot be expressed in the line's frame is refused by name.** The
  same object as the line's CRS needs no calculation. Otherwise the datum's CRS is
  read, and its projected and vertical systems must be provably the same as the line's
  (same EPSG code, same WKT, same authority); horizontal units that contradict each
  other, a unit stated on one side only, a CRS in time or absent are refused with the
  reason. Choosing one would be guessing.
- **The coordinates of a datum are read in one place**, which also reads the
  RESQML 2.0.1 form, where `Location` is the point and `LocalCrs` sits beside it.

## Consequences

- A view draws a `Described` portion like the rest of the well and an `Assumed` one
  in a style that says so; the studio's dashed grey portion is that second style.
- A trajectory of RESQML 2.0.1 whose line reaches MD 0 now has its head portion; one
  that starts below it is joined to the datum in the frame of its own nodes.
- A refusal names its reason (the datum or its CRS missing, another system, a CRS in
  time, contradictory units), and does not fall back on a drawing.
- The chord cannot be longer than the interval it spans; a check of that between two
  independent sources, the datum and the line, is left to a later warning rule rather
  than a refusal here.
