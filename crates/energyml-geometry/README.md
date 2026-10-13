# energyml-geometry

From an EnergyML representation to points, indices and a coordinate reference system. Representations are read through a registry of readers: meshes and surfaces, IJK grids (faults, K gaps, parametric pillars, local grids), unstructured grids, wellbore trajectories and frames, sub-representations, and geometry borrowed from another representation. Point arrays that are computed rather than stored (lattices, parametric lines, z-value arrays) are evaluated, not reported as empty. A representation the library cannot read is refused by name, never drawn empty.

`scene::of_package` reads a whole package into scene objects with their properties and colours, so every face gets the same answer.

## Features

| Feature | Default | Effect |
|---|---|---|
| `hdf5` | no | read arrays from HDF5 files, where most geometry lives; without it only forms that carry no data still evaluate |

## Documentation

- API reference: <https://docs.rs/energyml-geometry>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [library guide](../../docs/library.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
