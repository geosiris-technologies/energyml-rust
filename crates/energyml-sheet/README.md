# energyml-sheet

A drilling sheet: a well's 2D cross-section laid out in Rust and written as SVG. A depth column with a proportional axis, bands of stratigraphy or lithology, and property columns the user can choose and order. The layout is data and is tested in numbers; the SVG writer (`svg::write`) only serialises it. Text is measured against real font metrics so labels wrap correctly.

Depends on `energyml-core`, `energyml-domain` and `energyml-storage`; no native dependency.

One feature: `png` (resvg with its raster images, pure Rust) renders an SVG to PNG - a display list through its SVG, so the PNG always shows what the SVG shows; without it, those calls refuse by name. The `draw` module is the drawing every view shares (a keyed display list, its SVG, graduations, label spreading, rasters, a layered graph layout, the hit test); it knows no EnergyML type.

## Documentation

- API reference: <https://docs.rs/energyml-sheet>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [well sheet](../../docs/well-sheet.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
