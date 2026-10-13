# energyml-geostat

Domain-neutral interpolation and meshing algorithms, usable outside EnergyML (hydrogeology, environmental work, civil engineering). Inputs are plain samples, query points, coordinate triples and node indices; the crate depends on no `energyml-*` crate, has no feature flags and compiles to wasm.

Conventions: a `NaN` sample is set aside and counted, never averaged; refusals are named (no samples, mismatched lengths, invalid parameter) so a malformed call is never confused with an interpolated `NaN`.

## Modules

- Interpolation: `idw` (inverse-distance weighting), `kriging` (ordinary kriging with variance), `variogram` (empirical variogram and models), `rbf` (radial basis functions), `mba` (multilevel B-splines), `mba_frame` (MBA in a tilted local frame).
- Meshing and shapes: `triangulate` (2D Delaunay, exact predicates, constrained edges), `isocontour` (marching triangles), `resample`, `densify`, `loft`, `outline`, `extend`, `cells`, `nodes`.
- Tools: `measure` (areas, lengths, volumes), `barrier` (fault traces as visibility barriers), and a private dense LU solver.

`energyml-analysis` re-exports every module, so `energyml_analysis::kriging::...` keeps resolving.

## References

> **The citations below are pending verification.** They were written into the code by hand; none has yet been checked against its source (authors, year, title, venue, pages, DOI). Algorithms that name a method without a publication, or that cite none, carry a `TODO(reference): verify` line in their module documentation. Do not treat a citation here as authoritative until that line is gone.

Citations present in the code:

- `mba`: Lee, Wolberg and Shin, "Scattered Data Interpolation with Multilevel B-Splines", IEEE Transactions on Visualization and Computer Graphics 3(3), 1997, pp. 228-244 (DOI 10.1109/2945.620490); also mentions Boehm (1985) and Lyche and Morken for the refinement mask, and de Boor, *A Practical Guide to Splines*.
- `loft`: Christiansen and Sederberg (1978); Fuchs, Kedem and Uselton (1977).
- `extend`: Hoppe (1992) (principal components); Huber's weights.
- `densify`: Yuksel, Schaefer and Keyser (2011) (centripetal Catmull-Rom).
- `idw`: Shepard's method (named, no full citation). `outline`: Andrew's monotone chain (named). `isocontour`: marching triangles (named). `kriging`, `variogram`, `rbf`, `triangulate`: no publication cited.
- `cells`: RESQML Technical Usage Guide 2.2 (draft), section 9.12, for the IJ node ordering.

Where a method is developed here rather than taken from a publication, the module says so; nothing is cited that was not followed.

## Documentation

- API reference: <https://docs.rs/energyml-geostat>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
