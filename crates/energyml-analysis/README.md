# energyml-analysis

What is computed from EnergyML shapes, as opposed to what a file states (which is the job of `energyml-geometry`). Inputs are plain samples and query points: nothing here reads a package, so the crate compiles to wasm and works over any storage.

The domain-neutral algorithms (IDW, kriging with variograms, RBF, multilevel B-splines, triangulation, iso-contours, resampling, lofting, ...) now live in [`energyml-geostat`](../energyml-geostat/) and are re-exported from this crate, so existing paths such as `energyml_analysis::kriging::...` keep working. What stays here is the domain computation over wells and grids: surveys, well plans, tops, time-to-depth, fault throw and sticks, IJK grids and their faults, thickness, flow laws, step-rate and Hall tests, water chemistry and well logs.

Conventions: a `NaN` sample is set aside and counted, never averaged; refusals are named (no samples, mismatched lengths, invalid parameter) so a malformed call is never confused with an interpolated `NaN`. The result of an interpolation is meant to be written back as an EnergyML object by the caller.

There are no feature flags.

## References

> **The citations in the code are pending verification.** The references in the module documentation (Hall 1963, Piper 1944, Stiff 1951, Hem 1985, API Bulletin D20, Sawaryn and Thorogood SPE 84246, Singh, Agarwal and Krase SPE 16798, APHA Standard Methods 2340 B, ...) were written by hand and several are marked as cited from memory. None has been checked against its source yet. Algorithms without a reference are marked `TODO(reference): verify` in their module documentation. See also the list in [`energyml-geostat`](../energyml-geostat/README.md#references).

## Documentation

- API reference: <https://docs.rs/energyml-analysis>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [geostatistics and references](../../docs/geostat.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
