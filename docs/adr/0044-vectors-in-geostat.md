# 0044 - The vectors in geostat

Status: accepted

## Context

The cross product, the dot product, the norm and the distance were written in
more than a dozen private places: four modules of `energyml-geostat`, three of
`energyml-analysis`, `min_curvature` in `energyml-geometry`, three crates for a
distance, and a few 2D variants. `energyml_geometry::vector` claimed to be the
one place, but `energyml-geostat` depends on no EnergyML crate (and must stay so)
and could not use it.

## Decision

The module moves down to `energyml_geostat::vector`: 3D vectors on `[f64; 3]`
(`add`, `sub`, `scale`, `dot`, `cross`, `norm`, `distance`, `unit_above`, `unit`,
`distance_to_chord`) and, in `vector::plan`, the 2D companions on `[f64; 2]`
(`add`, `sub`, `scale`, `dot`, `cross`, `norm`, `distance`, `segment_distance`,
`distance_to_segment`, `orient`, `on_segment`, `segments_meet`).

`energyml-geometry` depends on `energyml-geostat` (pure Rust, no native
dependency, compiles to wasm) and `energyml_geometry::vector` is
`pub use energyml_geostat::vector::*`, so its public API does not change.
`energyml-geostat` still depends on no EnergyML crate.

A function of the same name that does something else (a three-point `cross`
giving an area, a vector product that is not the right-handed one) is not a
copy and keeps a name that says what it does.

## Consequences

- One definition of each helper; a change of handedness or of a floor is made
  once.
- `energyml-geometry` gains a dependency on `energyml-geostat`; the reverse
  never happens.
