# energyml-analysis — architecture

Related decisions: [ADR 0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR 0011](../../docs/adr/0011-tests-follow-the-specification.md), [ADR 0017](../../docs/adr/0017-operation-is-a-pure-function.md).

## Purpose and position

What is **computed from** EnergyML shapes, as opposed to what a file describes (that is `energyml-geometry`'s whole job). Geometry answers "what shape does this document state"; analysis answers "what can be derived from it": well plans and surveys, structural measures (thickness, dip, intersections), horizon and time-depth laws, IJK fault handling, flow and step-rate tests, water-chemistry diagrams, logs.

It sits above `energyml-geometry` and below `energyml-build` and the faces. It re-exports every module of [`energyml-geostat`](../energyml-geostat/ARCHITECTURE.md) (the domain-neutral algorithms: interpolation, meshing, iso-contours), so `energyml_analysis::kriging::…` and the other historical paths keep resolving.

## Rules

- **Nothing here reads a package.** Inputs are plain samples, query points, coordinates and indices. The crate compiles to WebAssembly with no feature flags, and a caller on any storage (EPC, ETP, none) uses it the same way. A result that should become an EnergyML object (a `ContinuousProperty` on a `Grid2dRepresentation`, say) is promoted by the caller, in `energyml-build`; the functions here never see a store.
- **Above geometry, never inside it.** Reading a surveyed hole is reading a shape and belongs to geometry (the minimum-curvature primitives live there); what is derived from it lives here.
- **A `NaN` sample is set aside, never averaged**, and the count of what was set aside is part of the answer. `NaN` is how the data says "no data here".
- **Refusals are named.** No samples, mismatched lengths, a non-positive power: each says what is wrong, because an interpolated `NaN` and "the call was malformed" are different news.
- **A calculation lives here, never in an operation.** An operation in `energyml-build` is the seam (read the bag, call the algorithm here, write through the shared writer). That keeps what can be wrong testable without a file.
- **A grid is measured as a lattice**: slope and azimuth by central differences on node positions (Zevenbergen and Thorne, 1987), never by cutting each cell along a diagonal the file did not choose. A hole node, and the nodes whose differences reach it, are `NaN` and counted.
- **Directions are read, not guessed.** Anything that depends on the sense of the vertical takes it as an argument.

## References

Each algorithm module cites, in its own documentation, the publication it follows (Hall 1963, Piper 1944, Stiff 1951, …). Citations not yet verified against the source are marked `TODO(reference): verify`.

## Tests

Tests state the rule on built data (`tests/profile.rs`, `ijk_faults.rs`, `step_rate.rs`, `water_diagrams.rs` and the unit tests of each module): no package, no HDF5. 