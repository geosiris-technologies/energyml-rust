# energyml-storage

Reads and writes EnergyML data stores behind one abstraction. `EnergymlStorage` is the mother trait, shaped after the ETP protocol (addressing by URI, plural operations, partial responses) rather than after a zip file; `EpcStore` implements it for EPC packages and also exposes a complete synchronous API, so an application that only reads local files never writes `async`. Arrays that live in external HDF5 files are located, read and written through pluggable backends.

The ETP implementation of the same trait lives in the sibling repository `retp`.

Design points: the registry of schema versions is passed in (nothing here names a version); a missing backend answers `FeatureNotEnabled`, never an empty array; one lookup door (`EpcStore::lookup`) resolves an identity to an object.

## Features

| Feature | Default | Effect |
|---|---|---|
| `hdf5` | no | read and write arrays in HDF5 files; needs the HDF5 C library at build time |

## Example

```rust
use std::sync::Arc;
use energyml_storage::EpcStore;

let registry = Arc::new(energyml_schemas::all());
let store = EpcStore::open("model.epc", registry)?;
for summary in store.resources() {
    println!("{}", summary.uri);
}
```

`energyml-schemas` supplies the registry; `resources()` lists the package's objects.

## Documentation

- API reference: <https://docs.rs/energyml-storage>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [library guide](../../docs/library.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
