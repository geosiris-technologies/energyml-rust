# energyml-geostat — architecture

Related decisions: [ADR 0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR 0011](../../docs/adr/0011-tests-follow-the-specification.md).

## Purpose and position

Domain-neutral interpolation and meshing algorithms: IDW, kriging and variograms, radial basis functions, multilevel B-spline approximation (MBA), 2D Delaunay triangulation, iso-contours, resampling, densification, lofting, outlines, cell and node tools, measures and barriers.

**It depends on no EnergyML crate.** Inputs are plain samples, query points, coordinate triples and node indices. It therefore compiles to WebAssembly with no feature flags and can be used in hydrogeology, environmental work or civil engineering as well as on EnergyML models. `energyml-analysis` re-exports every module, so older `energyml_analysis::…` paths keep resolving.

## Rules

- **No EnergyML knowledge, ever.** If an algorithm needs a type of the model, it belongs in `energyml-analysis`, not here; this boundary is what makes the crate reusable.
- **A `NaN` sample is set aside, never averaged**, and the count of what was set aside is part of the answer rather than a silent repair.
- **Refusals are named**: no samples, mismatched lengths, a power that is not a positive number are three different failures.
- **Triangulation uses exact predicates** (`spade`). On nearly collinear points, which a line of boreholes always is, floating-point predicates return overlapping triangles that no simple assertion would catch.
- **Duplicates are counted, not refused**, and collinear input is refused at computation time, not before: collinearity is only visible when triangulating.
- **A model nobody stated is reported.** A kriging without a variogram fits one and says so: an unstated model is a decision, not a default.
- **Absurd sizes are refused with their count** (an interpolation grid beyond ten million nodes), and a node out of reach stays a hole (`NaN`), never an extrapolated value presented as data.
- **The private linear solver (`solve`) is not part of the API.**

## References

Each algorithm module cites the publication it follows in its own documentation; citations pending verification are marked `TODO(reference): verify`.

## Tests

Unit tests inside each module, on constructed samples and never on a file. Tests of behaviour that depends on the model (writing the result into a package) live in `energyml-analysis` and `energyml-build`.
