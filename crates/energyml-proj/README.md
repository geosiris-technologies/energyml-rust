# energyml-proj

The reprojection backend for `energyml-domain`: pure Rust, wasm included, built on `proj4rs` for the arithmetic and `crs-definitions` for the EPSG table. A definition that says nothing about its datum is refused rather than applied, because a silent datum shift is an error of tens of metres.

```rust
use energyml_domain::reproject::{self, Wgs84};
use energyml_proj::Proj4;

let lonlat = reproject::to_epsg(crs, point, Wgs84::EPSG, &Proj4::new())?;
```

Here `crs` is an `energyml_domain::crs::Crs` and `point` an `[f64; 3]`.

There are no feature flags.

## Documentation

- API reference: <https://docs.rs/energyml-proj>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [library guide](../../docs/library.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
