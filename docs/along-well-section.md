# The geosteering section

A geosteering section is a vertical section **along a well**: the hole runs across the
page, and either side of it a cube of values (the resistivity of a deep-reading tool, for
instance) is laid out like a curtain. On top of the curtain sit the limits of the
tool's depth of detection, the horizons the curtain cuts, and the tops of the hole. It
lives in `energyml_section::along_well`, next to the long section of the
[section view](section-view.md) and not in it: a section of a model along a trace and a
section along a well have different geometry, so each has its own model and its own
drawing. The decisions are in [ADR 0038](adr/0038-geosteering-section.md).

The module is pure Rust, reads no file of its own and builds for WebAssembly.

## What it shows

| Layer | What it is | Where it comes from |
|---|---|---|
| curtain | one column per node of a wellbore frame, one row per offset, each cell a quadrilateral coloured by its value | a property with two dimensions per node |
| boundaries | the limits of the depth of detection above and below the hole | a property of radii and a property of angles |
| horizons | where each surface crosses the curtain, column by column | triangulated or gridded surfaces |
| tops | where the hole itself crosses each surface | the same surfaces |
| trajectory | the hole | the frame's trajectory |
| legend, notes | the scale of the values, and the sentences that say what was assumed or left out | the model |

Every layer is optional: a request with no curtain still draws the hole, its
boundaries, horizons and tops.

## One frame for everything

Everything is computed in **one frame**, the hole's. A point is `(md, dn)`: `md` is the
measured depth, `dn` the offset in metres along the high side of the hole, positive
up. **One function** carries a point into the drawing, `WellProjection::place` (see
[Around the well](around-the-well.md#laying-a-well-flat)). The cells, the
boundaries, the horizons and the tops all go through it, so a boundary at radius `r`
and the centre of the row at offset `r` are the same point, to the bit.

The projection is chosen by its id in the request: `md_tvd`, `developed` or
`vertical_section` (the last needs the azimuth and the origin of the plane, given in the
request). The vertical coordinate is the true vertical depth **below the MD datum**
([ADR 0033](adr/0033-tvd-below-the-md-datum.md)).

Where the hole is too close to vertical for a high side to exist (under one degree of
inclination, [ADR 0032](adr/0032-hole-frame-in-a-direct-placed-frame.md)), the nodes
there have no frame. They are counted and said in a note; they are never a refusal of
the whole section.

## The curtain

The curtain comes from a **source**, chosen by id in the request. The built-in
`cube-on-frame` reads a property carried by the nodes of a wellbore frame with a matrix
of two dimensions per node, each at least 2 (the UDAR `Rh` and `Rv` are `[81, 81]`). A
flat list of components is never factorised into a matrix.

What the file does not say, the request does (the rule of
[Around the well](around-the-well.md#angles-around-the-hole-a-declared-convention)):

- **The axes.** The offsets of each dimension come from the links the caller declares, then
  from the file's convention B (a table or a paired property, see
  [Tables, keys and links](tables.md)). A rank is not an offset: when only ranks are
  known the section is refused (`AxesUnknown`) and says which dimensions to declare.
  An axis must be in a unit of length.
- **The roles.** Which dimension runs along the high side, which is lateral, and the sign
  of each, are the caller's declaration (`roles`). The note "the roles and signs ... are
  the caller's declaration" is written on every curtain made this way.
- **The lateral row.** `lateral_index` picks the row of the lateral dimension that is
  shown. A plane that is off the hole is drawn in the plane of the hole and says so
  (a note), and only in `md_tvd` and `developed`: in any other projection it is refused
  (`LateralNotPlaced`).

The cells of a node reach half way to its neighbour, and never more than half the widest
step the gap rule bridges ([ADR 0027](adr/0027-gaps-seams-scales-and-quantiles.md)): a gap
in the data stays an empty band and is counted in a note, never bridged. A cell that folds
over the centre line of the section (in `developed`, where the hole bends sharply) is
**marked, never straightened**, and drawn where it falls. A stretch where the section
runs backwards is noted the same way. A value the file declares as `NullValue` is an
absence: the cell is left transparent and counted.

A source that is declared and not built (`grid-property`, `lwd-image`) refuses by name
(`SourceNotBuilt`), and so does an unknown id; neither draws an empty curtain. The
registry (`CurtainSources`) takes the caller's own sources first.

As an image, the curtain is painted by `curtain_image` into the pixels of the plot, the
cells farthest from the hole first, so that where two columns overlap the nearer one wins.
The colour comes from a closure the caller passes: the crate holds no palette, and a
face resolves the colour through the one door of colours
([How a colour is found](colour-inheritance.md)).

## Boundaries

The boundaries of a depth of detection are rings of radii at angles. They are found **by
the value of the angle**, never by its rank: an angle within 1e-6 rad of 0 is above the
hole, one within 1e-6 rad of pi is below. The angles may be given once for all the nodes
(the UDAR case) or node by node.

| Mode | What it draws |
|---|---|
| `in-plane` | the radii at the angles 0 and pi only, the points that fall in the plane of the section |
| `projected` | every angle, at `r cos(theta)` on the high-side axis (even in `theta`, so the sense of the angles is not needed) |

A radius that is NaN or negative gives no point and is counted. Two angles on one side of
one node are refused by name (which radius is the boundary is not decided here), and so
is an in-plane request where no angle is at 0 or pi. Radii and angles are converted from
their own units; a unit that is not a length or an angle is refused.

## Horizons and tops

No solver is written for the horizons. A column of the curtain is a line `P + dn h`, and
`energyml_analysis::tops::tops` finds where a line crosses a surface, `dn` playing the part
of the measured depth. Each crossing is placed by the projection, and the crossings of
neighbouring columns are joined when they pair (same number of crossings, same side). What
a column leaves undecided is kept, not smoothed: a fold of the surface, an unresolved
crossing, a gap, a touch. Places where crossings could not be paired are counted as breaks.

The range of offsets the horizons are looked for in is the request's `dnRange`, else the
curtain's rows, else the largest radius of the boundaries. With none of them, no horizon
is looked for and a note says so.

The **tops** of the hole are the crossings of the hole itself with each surface
(`computedTops` in the request), placed in the same drawing. The surfaces are given as
`PlacedMesh` values, read by `PlacedMesh::read` (the one door of the geometry, in metres)
or built by the caller.

## Markers

A marker is a point of the hole with, perhaps, a dip. `marker_ticks` computes the apparent
dip in the plane of the section, `tan(apparent) = tan(dip) cos(direction - azimuth)`,
where the azimuth is that of the hole at the marker's depth. A dip counted from true or
magnetic north is drawn level, and says so, unless the caller declares the angle to grid
north; against the measured depth the slope is multiplied by `sin(I)`.

There is **no reader of markers** in the library today (the well strata walk refuses a
marker set by name, and neither the RESQML marker frame nor the WITSML interval set has
one). So a request that names marker objects is not refused as a whole: each marker uri
is refused by name (`NoReader`, with the type the package holds), the section is drawn
without them, and a note counts them. A caller that holds markers by other means can pass
them to `marker_ticks` and put the ticks in `Model::markers`; the drawing does not
paint them yet, but the window takes them into account.

## What a frame offers

`along_well::offers(index, frame)` answers, from the package index alone (no array is
read), what a frame could show: the properties that make a curtain and what each still has
to be told (`Axes`, `AngleConvention`, or a choice between several angle siblings), the
radius and angle pairs that make boundaries, and the trajectory the frame references. These
are facts, not a choice: a face decides what to draw from them.

## Drawing, and drawing again

Three stages, three costs.

```text
Request --> build (reads, once) --> Model --> layout / draw (View) --> SVG
                                       +---> pick / pixel_of
```

- **`build`** is the expensive stage. It reads the frame, the trajectory, the columns,
  the boundaries and the maps of the surfaces, and places them.
- **`layout`** and **`draw`** only arrange the numbers of a `Model` under a `View`: the
  window, the vertical exaggeration, which layers are shown, the line colours, the legend.
  Changing the window or the exaggeration costs a drawing, not a reading. The
  display list is the shared one of [Drawing](drawing.md), and the SVG is written by
  its one writer.
- **`pick`** and **`pixel_of`** go from a pixel to the data and back: the measured depth
  and the offset (through the projection's way back), the cell and its value with its
  unit, the distances to the boundaries, and the nearest horizon of the column. `pick`
  gives the cell's own value by default; an interpolation of the domain's registry can be
  asked for, along the measured depth and never across the rows. Where a projection
  cannot be inverted (`plan`, `md_bar`), `pick` answers nothing.

The legend scale proposes a base-10 logarithm when the values span at least three decades
and takes the first and ninety-ninth percentiles as bounds, as everywhere else
([Statistics, scales and measured depth](statistics-and-depth.md)).

The SVG carries the **request and the view** in its metadata (element `along-well-view`).
`redraw` reads them back and makes the same drawing again over the same package, byte
for byte. A PNG is always the SVG rendered by `draw::png::render`, never a second
drawing. With `embedCurtain` the curtain is written into the SVG as an image, which needs
the `png` feature of `energyml-sheet`.

An operation, `along-well-view` of [Creating objects](creating-objects.md), stores the
drawing in a package on request: the SVG as an attachment of the frame and an `Activity`
that records what the section was computed from.

## What is refused

Every refusal is one sentence that names what is wrong.

| Refusal | When |
|---|---|
| `NotAUri` | a field of the request is not the full uri of an object (a bare uuid is turned into a uri at the edge, never here) |
| `NotInPackage` | an object, or a surface that was asked and not given, is not there |
| `NotOnFrame` | the frame is not a wellbore frame, or the property is not on it |
| `UnknownProjection`, `UnknownSource`, `UnknownInterpolation` | the id is not in the registry; the refusal lists what is |
| `SourceNotBuilt` | a source that is declared and not built |
| `NotTwoDimensions`, `FlatComponents` | the property is not a matrix of two dimensions per node |
| `AxesUnknown`, `AxisUnit` | the axes are ranks or labels, or not in a length |
| `LateralIndex`, `Curtain(LateralNotPlaced)` | the lateral row is outside its dimension, or a plane off the hole where it cannot be drawn |
| `Boundaries(...)` | the two properties are not on the same rings, no angle at 0 or pi, two angles on one side |
| `BoundaryUnit` | a radius that is not a length, an angle that is not an angle |
| `NoFaces` | a surface with neither triangle nor quadrilateral |
| `BadMetadata`, `Metadata` | a drawing that does not carry this section's request, or of another format |
| `Legend` | a legend that was asked and cannot be drawn |

What is not a refusal is counted and written beside the picture: cells with no value,
folds, backwards stretches, gaps, nodes with no high side, a lateral plane drawn in the
plane of the hole, markers that were not read, a curtain whose file colours only one
component.

## From Rust

```rust,no_run
use energyml_geometry::around_well::projection::WellProjections;
use energyml_section::along_well::{
    BoundaryAsk, BoundaryMode, CurtainAsk, CurtainSources, Request, View, build, draw,
};
# fn run(store: &dyn energyml_domain::ArraySource, ask: CurtainAsk, boundaries: BoundaryAsk) -> Result<(), Box<dyn std::error::Error>> {
let request = Request {
    frame: "eml:///resqml22.WellboreFrameRepresentation(...)".into(),
    trajectory: None,                       // the one the frame references
    curtain: Some(ask),                     // property, source id, declared axes and roles
    boundaries: Some(boundaries),           // radius, angle, mode
    surfaces: vec![],
    computed_tops: false,
    markers: vec![],
    md_range: None,
    dn_range: None,
    projection: "md_tvd".into(),
    section: None,
    step: None,
};
let projections = WellProjections::standard();
let model = build(&request, store, &[], &CurtainSources::builtin(), &projections)?;
let view = View {
    size: [1000.0, 600.0],
    window: None,
    exaggeration: None,
    show_curtain: true,
    show_boundaries: true,
    show_surfaces: true,
    show_tops: true,
    colours: Default::default(),
    legend: None,
    embed_curtain: false,
};
let svg = draw(&model, &view, &projections, None)?;
# let _ = (svg, BoundaryMode::InPlane);
# Ok(())
# }
```

Every field of the request that names an object is a full uri. Every fact the file does
not state (the roles and signs of the curtain, the axes when the file has none) is part of
the request, so the same request gives the same drawing.

See also [Around the well](around-the-well.md), [Section view](section-view.md) and
[Drawing](drawing.md).
