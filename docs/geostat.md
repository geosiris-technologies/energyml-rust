# Geostatistics and computation

`energyml-analysis` holds what is **computed from** EnergyML shapes, as opposed to
what a file states (which is `energyml-geometry`'s job). Interpolators, surface
meshing, structural and well computations all follow the same contract:

- inputs are plain slices of samples and query points; nothing here reads a
  package, so the crate builds for WebAssembly and works with any storage;
- a `NaN` sample is set aside and counted, never averaged;
- refusals are named: an interpolated `NaN` and a malformed call are different
  news;
- a function says what it measured and in which unit, and converts nothing the
  caller did not ask it to.

> Planned: the neutral part (interpolation, meshing, small tools; no `energyml-*`
> dependency) is to be split into its own crate, `energyml-geostat`, usable in
> hydrogeology, environmental or civil work, leaving the domain part in
> `energyml-analysis`. This page lists the modules by family; TODO(verify): module
> names after the split.

## Citations: what is and is not referenced

A reference is listed **only when the source code already cites it**, copied as
written there. Where the method is standard but the code names no publication,
the entry says `TODO(reference)`: a citation is added only after the work has
actually been checked against it. Some entries in the code say the source was
"cited from memory, no copy in this repository": those are kept as written and
flagged for verification.

## Interpolation

| Module | What it does | Reference |
|---|---|---|
| `idw` | Inverse-distance weighting (Shepard's method): the value at a point is the weighted mean of the samples, with an exponent and optional radius. Deliberately the plain interpolator; exact at samples | TODO(reference): Shepard's method is named in the code without a citation |
| `variogram` | The empirical variogram (mean pair distance per class, semivariance, pair counts) and fitted models, spherical and exponential. `range` is the *practical* range for the exponential model (about 95% of the sill) | TODO(reference) |
| `kriging` | Ordinary kriging with the kriging variance as the model's statement of uncertainty. The dense system is LU-factorised once; each query is a back-substitution | TODO(reference) |
| `rbf` | Radial basis function interpolation: thin-plate spline, multiquadric, linear, Gaussian kernels, with the polynomial term each kernel requires decided by the kernel and not exposed | TODO(reference) |
| `mba`, `mba_frame` | Multilevel B-spline Approximation: extends a surface while keeping its general shape where points are sparse, with a least-squares plane removed first (the paper's own initialisation). `mba_frame` fits in a tilted local frame for surfaces MBA cannot fit in world coordinates | Lee, Wolberg, Shin, *"Scattered Data Interpolation with Multilevel B-Splines"*, IEEE Transactions on Visualization and Computer Graphics, vol. 3, n° 3, 1997, p. 228-244 (DOI 10.1109/2945.620490) |
| `solve` | A dense LU factorisation with partial pivoting shared by kriging and RBF (the Lagrange border of kriging and the polynomial border of RBF make the systems symmetric but not positive definite) | - |
| `barrier` | Faults as barriers: a sample on the far side of a fault says nothing about this side | - (original method) |

## Surfaces and meshes

| Module | What it does | Reference |
|---|---|---|
| `triangulate` | 2D Delaunay and constrained Delaunay triangulation of a point cloud (faults as constraints), each point keeping its own z. Built on the `spade` crate for its exact geometric predicates, because floating-point predicates give overlapping triangles over nearly collinear points such as an alignment of boreholes | `spade` library; TODO(reference) |
| `isocontour` | Iso-lines of a per-node scalar over a triangulated surface. Inside one triangle a linear field crosses a level along one segment or not at all, so there is no ambiguous case; segments are then chained into polylines | TODO(reference) |
| `surface` | Dip, dip azimuth and curvature (mean, Gaussian, principal) of a triangulated surface, with the cotangent Laplacian over the mixed area | Meyer, Desbrun, Schröder and Barr (2003), *Discrete Differential-Geometry Operators for Triangulated 2-Manifolds*. TODO(reference): venue and DOI are not in the code |
| `resample`, `densify` | Resample a polyline at a spacing, or add points between vertices while keeping every vertex | - |
| `loft` | A surface between polylines (two lines, or successive sticks of a fault) | - |
| `outline` | The boundary of a triangulated surface, or the convex hull of anything else | - |
| `extend` | Extend a polyline or a surface past its border | - |
| `intersect` | Intersection of two triangulated surfaces, as polylines | - |
| `project`, `heights` | Vertical projection of nodes onto a surface or a level; exact heights of a surface under a plan position (triangles linear, quadrilaterals bilinear) | - |
| `nodes` | What erasing nodes or moving them does to a shape | - |
| `cells`, `ijk`, `ijk_faults` | Cells in an IJK box or crossed by a wellbore path; an IJK grid between horizons (pillars, K gaps); an IJK grid cut by faults (split coordinate lines, stated throw) | - |
| `measure`, `thickness`, `groups` | Areas, lengths and volumes of shapes; thickness between two horizons at every node; values split by a key (zone, stratigraphic unit) | - |

## Structural computations

| Module | What it does | Reference |
|---|---|---|
| `fault_throw` | A horizon triangulated with fault traces as constraints, torn along each trace, hanging wall offset by a throw that dies out at both ends | - |
| `fault_sticks` | Fault sticks drawn from a plane and the orientation of the surface they make | - |
| `horizon_law` | A horizon laid by a law over a reference surface | - |
| `t2d` | Analytic time-to-depth with a linear velocity-depth law `V(z) = V0 + k z`; `k = 0` is a legitimate constant-velocity law | TODO(reference) |

## Wells

| Module | What it does | Reference |
|---|---|---|
| `survey` | A survey read back from stations: inclination, azimuth and dogleg by the minimum curvature method, inverted | API Bulletin D20 (*Directional Drilling Survey Calculation Methods and Terminology*, 1985); Sawaryn & Thorogood, *A Compendium of Directional Calculations Based on the Minimum Curvature Method* (SPE 84246, SPE Drilling & Completion 20(1), 2005) - as cited in `well_plan` |
| `well_plan` | A wellbore from a drilling plan: straight holds and circular arcs, surveyed by minimum curvature | same as above |
| `tops` | Where a well crosses a surface (computed tops) | - |
| `profile`, `wells_profile` | Wells projected on a long profile: which heads fall in the band around a track, where along it, how far and on which side | - |
| `well_logs` | Values given by a law of depth along a wellbore (geothermal gradient, hydrostatic pressure) | - |
| `fake` | Fake dated data for tests: vertical polylines from a point cloud | - |

## Tests and hydraulics

| Module | What it does | Reference |
|---|---|---|
| `step_rate` | Which steps of a step-rate injection test have settled, and the injectivity index they give | - |
| `hall` | The Hall plot of an injection (cumulative pressure against cumulative volume; the slope is the inverse of the injectivity index under steady radial flow) | Hall, H.N. (1963), *How to Analyze Waterflood Injection Well Performance*, World Oil - cited from memory in the code, to be verified |
| `flow_law`, `time_law` | A flow rate over time and what it delivers between two instants; a value that drifts with time | - |

## Water chemistry

| Module | What it does | Reference |
|---|---|---|
| `ion_balance` | Whether a water analysis is electrically neutral: `100 (sum cations - sum anions) / (sum cations + sum anions)` in meq/L. The module computes; the tolerance is the caller's | molar masses: IUPAC standard atomic weights (abridged, CIAAW 2021), as cited in the code |
| `water_diagrams` | Piper and Stiff diagrams. An analysis that does not balance is refused before anything is drawn; an absent ion is never drawn as zero (the caller may *declare* an ion absent, and the result carries the declaration) | Piper, A.M. (1944), *A graphic procedure in the geochemical interpretation of water-analyses*, Transactions, American Geophysical Union 25; Stiff, H.A. Jr. (1951), *The interpretation of chemical water analysis by means of patterns*, Journal of Petroleum Technology 3; Hem, J.D. (1985), *Study and Interpretation of the Chemical Characteristics of Natural Water*, USGS Water-Supply Paper 2254 (all three "cited from memory" in the code, to be verified) |

## Testing the algorithms

Tests are unit tests over fabricated numbers. Where a publication gives a worked
numerical example, the plan is to reproduce it in a test with the numbers typed
in and the citation in a comment; nothing reads a file. TODO(reference): add such
tests as references are verified. A provenance audit (no code ported from a
licence incompatible with Apache-2.0) is part of the release checklist.
