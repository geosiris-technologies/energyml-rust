# energyml-views

The views of an EnergyML package, as pictures. One registry of views: each one says what it can show in a package (from the package index alone), declares the objects and the settings it takes, and draws a display list from what the other crates compute. One door checks a request against those declarations, draws, and writes the SVG with the request in its metadata, so that a drawing is made again, byte for byte, from the drawing alone. A PNG is always that SVG rasterised.

A view that cannot show an object refuses it by name; it never draws an empty picture. Colours come from the map the file gives, else from a palette that the legend names.

```rust,no_run
use energyml_views::{ViewAsk, ViewKinds, draw_svg};
# fn run(cx: &energyml_views::ViewContext<'_>) -> Result<(), energyml_views::ViewRefusal> {
let ask = ViewAsk::new("well-locator")
    .object("eml:///resqml22.WellboreFrameRepresentation(33333333-3333-4333-8333-333333333333)")
    .set("md", serde_json::json!(2950.0));
let picture = draw_svg(&ViewKinds::with_builtins(), &ask, cx)?;
std::fs::write("locator.svg", &picture.svg).ok();
# Ok(()) }
```

The command line asks the same door: `energyml view list`, `about`, `draw` and `redraw`.

## Features

| Feature | Default | Effect |
|---|---|---|
| `png` | no | rasterise a written SVG to PNG; without it, the PNG is refused by name |
| `hdf5` | no | read the arrays of HDF5 files, where most values live |

## Documentation

- API reference: <https://docs.rs/energyml-views>
- Repository documentation: [`docs/views.md`](../../docs/views.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
