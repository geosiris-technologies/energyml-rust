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
energyml view redraw package.epc --from-svg locator.svg --svg again.svg
```

- `--object` takes a full uri, or a bare uuid that the package index turns into one.
- `--set NAME=VALUE` is read by the nature the view declares: a number, a word of a list, `true`, or JSON.
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

## The views

| View | Objects | Shows |
|---|---|---|
| `well-locator` | a wellbore frame or trajectory | the well laid flat (`md_tvd`, `md_bar`, `developed`, `plan`, `vertical_section` with its `azimuth`), the interval the frame carries data over, and a mark at `md` |

## Colours

The map the package gives the object comes first, by the same walk as `energyml colors`. When it gives none, the
default palette of the value's reading paints (Viridis for a value, Set2 for a category), and the legend says the
colours are not from the file. `--set palette=NAME` chooses a palette by name; it wins over the file. The palettes
are quoted from their sources: Viridis from matplotlib (CC0), Spectral, YlGnBu, Set2 and Paired from ColorBrewer
(Apache 2.0).
