# energyml

The EnergyML library in one dependency. It re-exports the public crates of the [`energyml-rust`](https://github.com/geosiris-technologies/energyml-rust) workspace under short module names, so an application writes `energyml::storage::EpcStore` instead of adding fifteen dependencies and keeping their versions in step. It has no code of its own.

```toml
[dependencies]
energyml = { version = "0.1", features = ["all"] }  # or name the versions you read, see Features
```

| Module | Crate |
|---|---|
| `energyml::core` | `energyml-core`: runtime for the generated code (XML/JSON, reflection, validation, identifiers) |
| `energyml::schemas` | `energyml-schemas`: every generated schema version in one registry |
| `energyml::storage` | `energyml-storage`: EPC read, edit and rewrite behind one trait |
| `energyml::domain` | `energyml-domain`: units, CRS, value arrays, property kinds, colours, tables |
| `energyml::geometry` | `energyml-geometry`: representations to points and topology |
| `energyml::export` | `energyml-export`: OBJ, GeoJSON and VTK through a registry |
| `energyml::proj` | `energyml-proj`: reprojection between EPSG systems |
| `energyml::cache` | `energyml-cache`: what a package keeps of itself |
| `energyml::analysis` | `energyml-analysis`: computation on shapes |
| `energyml::geostat` | `energyml-geostat`: domain-neutral interpolation and meshing |
| `energyml::build` | `energyml-build`: create objects, operations and plans (feature `build`) |
| `energyml::providers` | `energyml-providers`: query public geoscience APIs (feature `providers`) |
| `energyml::rules` | `energyml-rules`: the library's own decisions, as a registry |
| `energyml::sheet` | `energyml-sheet`: a drilling sheet as SVG |
| `energyml::section` | `energyml-section`: long and cross sections as SVG |
| `energyml::log` | `energyml-log`: the one `tracing` subscriber a front end installs |

## Features

None is on by default: choose the schema versions you read.

```toml
# RESQML 2.2 and the EML 2.3 it is built on, nothing else
energyml = { version = "0.1", features = ["resqml22"] }
```

- Versions, one feature each, spelled as a qualified type is: `eml20`, `eml21`, `eml22`, `eml23`, `resqml20` (2.0.1), `resqml22`, `resqml22dev3`, `witsml20`, `witsml21`, `prodml20`, `prodml22`, `prodml23`. A version brings the EML version it is built on.
- Families: `eml`, `resqml`, `witsml`, `prodml`; and `all`.
- A version that is not compiled is a named refusal ("RESQML 2.2 is not compiled in: enable the `resqml22` feature…"), never a half read.
- `build` provides `energyml::build` and turns on RESQML 2.2, EML 2.3, WITSML 2.1 and PRODML 2.3; `providers` provides `energyml::providers` (implies `build`).
- `hdf5` reads and writes external arrays in HDF5; it needs the HDF5 C library at build time.
- `png` renders a section view to PNG.

The generated schema crates are reached through `energyml::schemas`. The `energyml` command is the separate crate `energyml-cli`.

## Documentation

- API reference: <https://docs.rs/energyml>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [library guide](../../docs/library.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
