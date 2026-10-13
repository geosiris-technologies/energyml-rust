# energyml-proj: architecture

## Purpose and position

The one `Reprojector` the workspace ships, built on `proj4rs` (arithmetic) and `crs-definitions` (the EPSG table). Pure Rust, wasm included. Nothing above depends on it: `energyml-domain` defines the `reproject` seam and `energyml-export` takes a `&dyn Reprojector`, so a face wires this crate in at the edge.

Related decisions: [ADR-0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR-0007](../../docs/adr/0007-core-has-no-native-dependency.md).

## Rules

**A definition that states no datum is refused, not applied** - applying it would be a silent datum shift, an error of tens of metres that looks like a coordinate. The same holds for a definition that asks for a correction grid: this backend has none.

**Z is returned intact** - a 3D Helmert transform would shift it by tens of metres, and a RESQML Z is not an ellipsoidal height. Only the horizontal position is reprojected.

**Agreement with PROJ, measured** - results are held to agree with PROJ 9.5.1 to the millimetre on a set of ten systems. Anything this backend cannot do equivalently is a refusal, not an approximation.

## Deliberately not here

No grid-shift support, no vertical transformation, no dependency on a C library (which would end wasm support).

## How it is tested

`tests/conformance.rs` compares results with reference values taken from PROJ for a set of systems, written into the test as constants; `tests/reproject_placed.rs` checks the seam with a placed local CRS. Expected values are quoted from the reference tool and its version, not read from a file at run time.
