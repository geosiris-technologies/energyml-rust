# energyml-build

Creates EnergyML objects through one door. An operation is a pure function from a bag of inputs (`Pieces`) to a `Plan` of objects; `apply` is the only step that touches storage. `Registry::offers` answers "what can I create from this?" with every refusal named, and each operation's parameters are projected into its canonical `ActivityTemplate`, so the form and the record are one declaration. Identities are derived deterministically, so replaying a recipe yields the same package.

Operations include creating a CRS, wells from tabular or JSON input, point sets from GeoJSON, contacts from wells, surface interpolation, and importing GeoSciML-style units; recipes chain operations into one package.

## References

The operations reuse the algorithms of `energyml-geostat` and `energyml-analysis` (interpolation, MBA, kriging, iso-contours, loft, densify, extend, triangulation, fault throw, horizon, time and flow laws, minimum curvature, water-analysis checks). Their references are listed in each operation's module documentation under `# References`; they are carried over as written in those crates and are pending verification (`TODO(reference): verify`). Where no published source is known, the module says so rather than inventing one.

## Features

| Feature | Default | Effect |
|---|---|---|
| `hdf5` | no | write point arrays (forwards `energyml-storage/hdf5`) |

## Documentation

- API reference: <https://docs.rs/energyml-build>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [creating objects](../../docs/creating-objects.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
