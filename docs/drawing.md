# Drawing: display lists, SVG, PNG, graduations and graph layout

`energyml_sheet::draw` is the drawing every view shares: what a view decides to
draw is a `DisplayList`, and the SVG, the PNG and the answer to "what is under
this point" all come from that one value. It is also **not** a viewer of
EnergyML: it names no EnergyML type, and a colour is a closure the caller
passes, never a colour map read here. The decision is
[ADR 0028](adr/0028-one-drawing-for-every-view.md).

## A display list

A `DisplayList` has a size and a unit (pixels, or millimetres on paper) and
layers, from back to front. A layer holds elements; an element is one thing a
user can point at, with an optional key (a full uri, a uri and a row...), a
kind, a tooltip and the primitives it is drawn with: a path, a thick polyline, a
rectangle, a polygon, a circle, a ring sector, a symbol, a line of text, a
raster. Everything is in user units, y down.

```rust
use energyml_sheet::draw::{
    Colour, DisplayList, Element, Item, Layer, PageUnit, Paint, Rect, Stroke, hit, svg,
};

let mut list = DisplayList::new([200.0, 100.0], PageUnit::Pixel);
let mut layer = Layer::new("data");
layer.elements.push(Element::keyed(
    "eml:///dataspace('d')/resqml22.MdPointSet(1)",
    "bar",
    vec![Item::Rect {
        rect: Rect { x: 10.0, y: 10.0, w: 50.0, h: 30.0 },
        corner: 0.0,
        fill: Some(Paint::Solid(Colour::rgb(200, 80, 40))),
        stroke: Some(Stroke::solid(Colour::rgb(0, 0, 0), 1.0)),
    }],
));
list.layers.push(layer);

let svg_text = svg::write(&list, None)?;
let under_the_pointer = hit(&list, 30.0, 20.0).map(|h| h.key);
```

A list is checked before anything is written (`DisplayList::validate`): a
coordinate that is not a finite number, a negative width or a malformed name is
refused by name. Writing it as 0 would draw a wrong point.

## The SVG

`svg::write` writes one group per element, with its key as `data-key`, colours as
attributes, everything escaped, and an optional `<metadata>` block the caller
fills (the body is the caller's own text; this module never parses it).
`svg::read_metadata` reads it back, so that a view can be redrawn from its own
export.

## The PNG, and the `png` feature

A PNG is **always the SVG rasterised**: `png::render(svg, scale)` is the only way
to one, and `png::render_list` is just `render(&svg::write(list, None)?, scale)`.
The PNG therefore always shows what the SVG shows. It needs the `png` feature
(`resvg`, pure Rust, WebAssembly included, with the font embedded: no system
font, the same picture on every machine); without the feature, the call refuses
by name. The rasters a view embeds in its SVG are drawn through the `raster-images`
feature of `resvg`, with an image resolver that accepts only a PNG embedded in a
`data:` URL: any other image is refused by name (not a PNG, not embedded,
unreadable, too large), never left out of the picture. The feature also compiles
GIF, WebP and JPEG decoders, which are never reached.

## Graduations

`draw::ticks` is the one rule for round numbers on an axis.

- **Linear** (`nice_ticks(a, b, target)`): a step of 1, 2 or 5 times a power of
  ten, the smallest one that gives no more than `target` intervals (a ruler that
  crowds is worse than one that is sparse). Values are `k x step`, never
  accumulated, each the number nearest to its decimal (`0.3`, not
  `0.30000000000000004`), never `-0`. Bounds in either order; refusals by name:
  a bound that is not finite, equal bounds, a target below one.
- **Logarithmic** (`log_ticks`): 1, 2 and 5 times each power of ten if they fit
  the target, else the powers of ten, else one power in every few. Positive bounds
  only.
- **`label`** and **`log_label`** write a graduation with the decimals its step
  needs. **`nice_step_at_most`** and **`nice_step_at_least`** give the round step
  just under or just over a length, exact as a decimal (a scale bar).

This is the rule of the cross-section and of the studio. The desktop used to take
the round step nearest to the ideal one, which draws more graduations on some
ranges; it will draw fewer, and say so.

## Spreading labels

`spread::spread(positions, gap)` pushes labels apart by at least `gap`, **only
towards larger values** and without changing their order. A position that is not
a finite number is not placed (it comes back as `NaN`) and moves nobody. Nothing
is clamped to a drawing area: the caller compares the result to its own height.

## Measuring text

`draw::text` measures and wraps text against the table of Liberation Sans advances
(`string_width_pt`, `wrap`); `energyml_sheet::text` re-exports it. It works in the
unit of the size it is given.

A text has a weight (`TextStyle::weight`, `FontWeight::Normal` by default, `Bold`
written as `font-weight="bold"`; the PNG follows because it rasterises the SVG, and
embeds the regular face only). The table holds regular advances, so a bold text is
measured `BOLD_WIDTH_FACTOR` (1.1) times wider: it errs toward a line too short.

`fit_to_page(&mut list)` breaks every unrotated text that would leave the page
(past `list.size[0]` or before 0, against its anchor) into lines that fit, with
`wrap`; the first line keeps its baseline, the next ones are 1.25 sizes lower. A text
that fits is untouched. `max_text_x(&list)` is the measure a test holds a view to.
`energyml-views` applies it at the one door of a picture (`draw_list`), and breaks
the band of notes under the picture itself, so the page grows by its lines.
Rotated texts (axis titles) are not broken.

## Hit testing

`hit(&list, x, y)` answers with the topmost keyed element under a point: last
layer, last element, last item, lines held within `HIT_SLOP` (3 units) of
their axis. A face that points with a mouse uses `hit_within(&list, x, y, grab)`
instead, where `grab` is the grab distance in units of the page (pixels for a
page in pixels; 0 or a value that is not a finite positive number means 0).

What `hit_within` holds: a fill (a rectangle, a polygon, a path, a sector, an
image) that contains the point, or a line, an outline, a disc, a symbol or a
text at no more than `grab` from the point (plus half the stroke width for a
line or an outline). The rule between several candidates:

1. a line, an outline, a disc, a symbol or a text held within `grab` beats a
   fill that contains the point;
2. then the highest layer;
3. then the nearest (a fill that contains the point is at distance 0);
4. then the element drawn last.

An element without a key is never held; a layer's clip holds nothing outside it.
`Hit` carries the layer, the element, the key and, on an image, the pixel.
`hit` itself is unchanged (plain order of drawing, its own slop): its answers
differ from `hit_within(.., 0.0)` when a fill is drawn over a line.

`outline(&list, layer, element)` gives the SVG path commands (`M`, `L`, `Z`, two
decimals, units of the page) that trace an element: rectangles, polylines,
polygons, flattened paths, discs and sectors in segments, symbols, images (their
rectangle) and texts (their box measured by `draw::text`, rotated like the text).
A face strokes it over the picture to highlight the element; `None` for an index
out of range or an element with nothing to trace.

## Rasters

`draw::raster` paints value grids and quadrilaterals into an RGBA image through a
colour function of the caller, keeps gaps as gaps (a gap larger than a cap is
never bridged), and counts what it could not colour.

## A layered graph

`draw::layered` places a directed graph in layers, deterministically: back edges
are found first, nodes are put in layers (two methods in a registry the caller can
extend, whose additions are tried first), ordered inside layers to reduce
crossings (never worse than the initial order), positioned, and the edges are
routed as cubics. Pins keep a node where the caller asked. A graph beyond its
bounds, an edge naming an absent node or a method that breaks the layer order is
refused by name.

## What moves here later

The cross-section keeps its own list for now: its nested groups, zoom attributes
and byte-for-byte tests need more than a `DisplayList` offers today, so it
migrates in a lot of its own. Until then, a few second implementations remain
(XML escaping, the PNG renderer of the cross-section, the font file), and are
listed in the ADR.
