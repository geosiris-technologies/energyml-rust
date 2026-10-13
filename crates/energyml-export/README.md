# energyml-export

Writes EnergyML geometry to interchange formats through a registry: an `Exporter` implementation is added to an `Exporters` registry, and entries pushed by the caller are tried before the built-ins. Built-ins are OBJ, GeoJSON and VTK (three flavours). What each format carries beyond geometry is configuration (metadata specs), not code; GeoJSON refuses what it cannot represent faithfully.

```rust
use energyml_export::{Exporter, Exporters};

fn extend(mine: Box<dyn Exporter>) {
    let mut exporters = Exporters::builtin();
    exporters.push(mine); // found before the built-ins
}
```

## Features

| Feature | Default | Effect |
|---|---|---|
| `hdf5` | no | forwards `energyml-geometry/hdf5` and `energyml-storage/hdf5` |

## Documentation

- API reference: <https://docs.rs/energyml-export>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [library guide](../../docs/library.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
