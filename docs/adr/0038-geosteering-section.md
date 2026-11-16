# 0038 - The geosteering section

Status: accepted

## Context

A geosteering section lays a cube held on the nodes of a wellbore frame (the
resistivity of a UDAR tool, for instance) either side of the hole, with the
boundaries of its depth of detection, the horizons it cuts and the tops of the
hole. Four things the files leave open, or that two implementations would
settle differently, had to be decided once.

## Decision

- **The curtain lives in `energyml_section::along_well`**, next to the long
  section and not in it: it has its own model (`Request`, `Model`) and its own
  drawing (`energyml_sheet::draw`), and adds no dependency to the crate.
- **One frame, one door.** Everything is computed in the frame of the hole: a
  point is `(md, dn)`, `dn` the offset along the high side, positive up. One
  function carries it into the drawing, `WellProjection::place`. The cells, the
  boundaries, the horizons and the markers all go through it, so a boundary at
  radius `r` and the centre of the row at offset `r` are the same point.
- **Folds are marked, never straightened; gaps are never bridged.** A cell
  that folds over the centre line in `developed` (where `dn I'` reaches 1) is
  flagged and drawn where it falls; the cells of a node reach half the way to
  its neighbour, at most half the widest gap the rule bridges (S7).
- **What the file does not state is declared by the caller.** The dimension
  along the high side and the sign of each dimension are the request's
  (S2); the axes are the caller's links, then the file's convention B, and a
  rank is never an offset (refusal `AxesUnknown`). A flat list of components is
  never factorised into a matrix.
- **Angles of the boundaries are searched by value**, within 1e-6 rad of 0
  (above) and pi (below), never by rank; two angles on one side of a node, or
  none in the plane, are refused by name.
- **A node with no high side (S4) is counted, not a refusal of the section.**
  A source that is declared and not built, and a marker (no reader exists),
  are refused by name and the section is still drawn.
- **The drawing carries its request and its view** in its metadata, and is made
  again from them byte for byte. A PNG is the SVG rendered by
  `draw::png::render`, never a second drawing.

## Consequences

- A face asks `build` once and `layout`/`draw`/`pick` as often as the view
  changes; nothing is read again.
- A new kind of curtain source is an entry of the `CurtainSources` registry,
  the caller's tried first.
- Reading the markers of a well is left to a future reader: the pure function
  `marker_ticks` exists and is tested.
