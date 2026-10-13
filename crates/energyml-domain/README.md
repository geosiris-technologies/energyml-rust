# energyml-domain

The EnergyML concepts that are the same in every version, implemented once over the generated types by reflection, so RESQML 2.0.1, RESQML 2.2, WITSML and PRODML share one implementation.

| Module | What it answers |
|---|---|
| `units` | unit conversion (how much is `1000 ft` in metres) |
| `crs` | where an object is placed and which way z counts |
| `reproject` | reprojection seam (implemented by `energyml-proj`) |
| `values` | the numbers of any array form, including constant and lattice arrays that carry no data |
| `stats` | extremes, distribution, share of `NaN` |
| `property`, `property_kind` | what a property hangs on, what kind of quantity it is |
| `color`, `table` | how an object is coloured, what a discrete code means |
| `well_strata`, `series` | stratigraphy along a well, log and time series |

## Features

| Feature | Default | Effect |
|---|---|---|
| `hdf5` | no | forwards `energyml-storage/hdf5`; only the tests need it |

## Data shipped in this crate

Three tables live under `data/` and are embedded at build time. The build fails,
naming the file, if one is missing or unreadable, and `cargo package` includes
them so the published crate is self-contained.

| File | What it is | Source and licence |
|---|---|---|
| `data/property_kinds.tsv` | the PropertyKind dictionary (3 662 kinds), compacted to one tab-separated line per kind | Energistics `PropertyKindDictionary` v2.3 (eml 2.3). Subject to the Energistics Product License Agreement, <https://www.energistics.org/product-license-agreement>. Provenance and format in `data/property_kinds.NOTICE.md`; the notice is repeated in the root `NOTICE` |
| `data/geolts/` (`GeolTs.csv`, `GeolTsBound.csv`) | the chronostratigraphic chart used for stratigraphic ages | BRGM, register GeolTs, <https://data.geoscience.fr/ncl/GeolTs>, CC BY 4.0. Attribution "BRGM - GeolTs"; the data is distributed unmodified and the crate only compacts it at build time. Terms in `data/geolts/LICENSE.md` |
| `data/activityTemplates.json` | the canonical `ActivityTemplate` of each operation of the object-creation engine, with fixed identifiers | written by this project, Apache-2.0 like the code |

Anyone redistributing a binary that embeds the chart must keep the BRGM
attribution (see the root `NOTICE`).

## Documentation

- API reference: <https://docs.rs/energyml-domain>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [library guide](../../docs/library.md), [house rules](../../docs/house-rules.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
