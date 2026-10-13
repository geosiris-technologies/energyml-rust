# energyml-section

Long and cross sections of a model along a trace: wells projected, surfaces cut, grids sliced. Three stages with three costs: `read::contribution` reads one package into a contribution (the only stage that touches a store), `assemble::assemble` joins contributions into a `Catalogue`, and `layout::layout` turns a catalogue and a `View` into a `Layout` of scales, ticks, fills and labels, which `svg::write` serialises. Re-laying out a view recomputes nothing upstream.

## Features

| Feature | Default | Effect |
|---|---|---|
| `png` | no | `png::render` rasterises the SVG with `resvg`, with an embedded font; without it the call refuses by name and says how to enable it |

## Documentation

- API reference: <https://docs.rs/energyml-section>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [section view](../../docs/section-view.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
