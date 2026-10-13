# energyml-rust (Python)

Read, edit and write EnergyML packages (EPC and their HDF5 files) from Python, over the Rust workspace. Install the wheel `energyml-rust`; import it as `energyml.rust`.

```sh
pip install energyml-rust
```

```python
from energyml.rust.epc import Epc
from energyml.rust.resqml import geometry_of, properties_of

pkg = Epc.open("model.epc")
for one in pkg.find("TriangulatedSetRepresentation"):
    shape = geometry_of(pkg, one.uri)          # points and topology, placed
    values, problems = properties_of(pkg, one.uri)
```

- **No h5py needed.** The HDF5 library is bundled in the wheel; arrays reach numpy by copy.
- **No generated Python classes.** Objects cross as dictionaries carrying their `$type`; `energyml.rust.model` describes the generated model (types, fields, enumerations) and builds empty objects to fill.
- **Beside the other `energyml.*` distributions**: everything installs under `energyml/rust/` and `energyml/` is a namespace package, so it coexists with other `energyml.*` wheels in one environment.
- The bridge carries no feature the Rust libraries do not have; it only converts.

| Module | Contents |
|---|---|
| `energyml.rust.storage` | the abstraction above every store |
| `energyml.rust.epc` | open, create, list, query, read, edit and write an EPC and its arrays; reference graph; `check` |
| `energyml.rust.model` | `types`, `describe`, `template` |
| `energyml.rust.resqml` | geometry, properties, export to OBJ, CSV, GeoJSON, VTK |

ETP is not part of this wheel.

## Building from source

A maturin project (`pyproject.toml`, native module `energyml.rust._core`). Cargo features: `python` (what maturin enables: pyo3, numpy and HDF5) and `hdf5` (the backend alone); nothing is on by default. The Rust crate is not published to crates.io (`publish = false`); the wheel goes to PyPI. Requires Python 3.9 or later (one `abi3` wheel per platform) and numpy 1.21 or later.

## Documentation

- [Python guide](../../docs/python.md) and [architecture](../../ARCHITECTURE.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
