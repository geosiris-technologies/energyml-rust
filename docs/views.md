# Views as pictures

`energyml-views` draws the views of a package as pictures: SVG, and PNG on demand. One registry of views, one door,
the same picture for every face (the command line, a desktop window, a web page). The decisions are in
[ADR 0043](adr/0043-the-views-of-a-package-through-one-door.md).

## From the command line

```sh
energyml view list  package.epc                      # what each view can show, from the index alone
energyml view about well-locator                     # its objects and its settings, with their defaults
energyml view draw  well-locator package.epc \
    --object "eml:///resqml22.WellboreFrameRepresentation(9ccdea13-638d-5d72-8c08-c82163596218)" \
    --set mode=plan --set md=3050 \
    --svg locator.svg --png locator.png --png-scale 2
energyml view draw  along-well package.epc --proposed \
    --object "eml:///resqml22.WellboreFrameRepresentation(9ccdea13-638d-5d72-8c08-c82163596218)" \
    --object "eml:///resqml22.ContinuousProperty(13568c0a-a6f8-593a-a171-83828de41874)" \
    --svg section.svg          # a first drawing, the proposals said as notes
energyml view redraw package.epc --from-svg locator.svg --svg again.svg
```

- `--object` takes a full uri, or a bare uuid that the package index turns into one.
- `--set NAME=VALUE` is read by the nature the view declares: a number, a word of a list, `true`, or JSON.
- `--proposed` fills the settings you did not give with what the view proposes (see [Proposals](#proposals)); a
  setting given with `--set` is kept. Each proposal is written on stderr as a note, `proposed: NAME=VALUE - why`.
- `--png` needs a build with `--features png`. It is the SVG just written, rasterised.
- A refusal says which object, which setting and why, and nothing is written. A part the view left out, with the
  rest drawn, is a note: on stderr, and in a band under the picture.

## From Rust

```rust,no_run
use energyml_views::{ViewAsk, ViewKinds, draw_svg};
# fn run(cx: &energyml_views::ViewContext<'_>) -> Result<(), energyml_views::ViewRefusal> {
let ask = ViewAsk::new("well-locator")
    .object("eml:///resqml22.WellboreFrameRepresentation(9ccdea13-638d-5d72-8c08-c82163596218)")
    .set("mode", serde_json::json!("md_tvd"))
    .set("md", serde_json::json!(3050.0));
let picture = draw_svg(&ViewKinds::with_builtins(), &ask, cx)?;
# Ok(()) }
```

The context (`ViewContext`) holds the package (an `ArraySource`), its index (`PackageIndex`), the colour door
(`PackageColours` over `energyml_cache`, or `NoColourMaps`) and the palettes. A face that draws the display list
itself asks `draw_list`; a view of the caller's is registered with `ViewKinds::register` and tried first.

## Proposals

Some values are not in the file and a view refuses to draw without them (the roles of the two dimensions of a
curtain, the table that holds its coordinates). A **proposal** is a value the view offers for such a setting, from the
package alone, so that a first drawing exists and can then be corrected: `assist(&kinds, &ask, cx)` returns the
proposals (`Proposal { setting, value, why }`) and the dimensions the view needs declared (`DimensionAxes`: the
dimension, its size, the settings that hold its role, and the numeric columns of the package's tables that have
exactly that many rows, as `AxisColumn`).

A proposal is never presented as read from the file. It is always said where it shows: the command line writes a
note for each, a form marks the field as proposed and shows the reason, and a person's own value replaces it. A view
with nothing to propose (the hole of `along-well`, with no property) answers an empty `Assist`.

For the curtain of `along-well`:

- `curtain_links` is proposed when the file links no axis and **one** table of the package goes with the curtain
  (one key column, one numeric value column per dimension, the right number of rows): dimension `i` takes the
  `i`-th value column ([ADR 0029](adr/0029-convention-b-links-by-qualifier-facet.md)). With two such tables nothing
  is proposed, and both are candidates for the person to choose.
- `slow_axis=high_side`, `slow_sign=positive`, `fast_axis=right`, `fast_sign=positive` are a declaration: the file
  does not state the roles of the dimensions.
- `lateral_index`, left out, is the middle row of the lateral dimension (the one not along the high side), not row
  0, which is the edge of the cube and off the plane of the hole.

`axis_links_value` writes the links setting from `(dimension, column)` pairs and `axis_links_read` reads it back, so
that a face never writes the JSON shape itself: `[{"table": {"table": uri, "column": title, "dim": n}}, ...]`
(a link may also be a `paired-property` or a `witsml-channel`).

For the target (`target`): `md` is the first node of the frame (`frame_mds`), `origin=high_side`, `sense=clockwise` and
`viewed_from=looking_downhole` are a declaration of the angle convention, and the sectors are `regular_sectors=true`
only when no property of angles pairs with the property on the frame; when exactly one does, it is proposed as
`angles` instead. With several, nothing is proposed: the offer names them as a choice. The drawing says the
proposals in its notes, like any other.

## Names, parts and what an offer proposes

A view has a short readable name (`ViewKind::name`, `ViewKinds::name_of(id)`: `Along the well`, `Target`), what a
face shows instead of the technical id. An `Offer` adds two fields to the objects and the `title`:

- `part`: what the view shows of its objects when it is not the whole object (`the hole`, `the curtain Rh`);
  `None` for the object itself. A face puts it after the name of the view, never the title of the object it is
  already looking at.
- `proposed`: what the request should give but that `assist` proposes, in words. These entries are **not** in
  `missing`, which keeps only what nobody proposes. An offer is made from the index alone, so it says
  `proposed` only for what `assist` is sure to propose: the roles of a curtain, its links when one table of the
  package goes with it (the row count of each table must be known without reading an array), the convention, the
  sectors, and the first node as `md` when the property hangs on a wellbore frame.

`energyml view list` prints the name, the part, then a `proposed` line and a `needs` line for each entry.

## The views

| View | Objects | Shows |
|---|---|---|
| `well-locator` | a wellbore frame or trajectory | the well laid flat (`md_tvd`, `md_bar`, `developed`, `plan`, `vertical_section` with its `azimuth`), the interval the frame carries data over, and a mark at `md` |

## Text stays on the page

No text of a view leaves its picture: `draw_list` breaks a title, a label or a legend name wider than the page into
lines (`energyml_sheet::draw::text::fit_to_page`), and the notes under the picture are broken to the page's width,
the band growing by its lines. `md-coverage` writes the name of a bar too short for it beside the bar, above it when
the band has the room, else below. `property-insight` writes the eight strongest seams of an array (the step over the
median step) and counts the rest in one line, `and K more seams`.

## Colours

The map the package gives the object comes first, by the same walk as `energyml colors`. When it gives none, the
default palette of the value's reading paints (Viridis for a value, Set2 for a category), and the legend says the
colours are not from the file. `--set palette=NAME` chooses a palette by name; it wins over the file. The palettes
are quoted from their sources: Viridis from matplotlib (CC0), Spectral, YlGnBu, Set2 and Paired from ColorBrewer
(Apache 2.0).
