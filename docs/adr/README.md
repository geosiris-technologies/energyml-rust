# Architecture decision records

Short records of decisions that shape this library and the reasons behind them.
Each one has three parts: **Context** (what forced a choice), **Decision** (what
was chosen) and **Consequences** (what it costs and what it buys). They are
written to be read in a minute; the code and the other pages are the detail.

A record is not edited to change its meaning. A decision that is reversed gets a
new record that supersedes the old one, which keeps its text and a status line.

| # | Decision |
|---|---|
| [0001](0001-generated-code-is-committed.md) | Generated code is committed, produced by an internal tool |
| [0002](0002-storage-trait-follows-etp.md) | The storage trait is drawn after ETP |
| [0003](0003-identity-full-uri-one-door.md) | Identity: full URI internally, one door per access |
| [0004](0004-reading-policy.md) | Reading policy: `ReadMode`, version resolution, tolerance |
| [0005](0005-refuse-rather-than-draw.md) | Refuse rather than draw; `FeatureNotEnabled` |
| [0006](0006-registries-rather-than-match.md) | Registries rather than `match` |
| [0007](0007-core-has-no-native-dependency.md) | `core` has no native dependency (WebAssembly) |
| [0008](0008-inheritance-and-xsd-list.md) | Inheritance and `xsd:list` |
| [0009](0009-properties-tables-colours.md) | Properties, tables and colours: one colour rule |
| [0010](0010-package-index-and-caches.md) | The package index and caches |
| [0011](0011-tests-follow-the-specification.md) | Tests follow the specification, not a corpus; triangulation; a test must be able to fail |
| [0012](0012-no-face-carries-logic.md) | No face (Python, CLI) carries logic |
| [0013](0013-witsml-channels-in-hdf5.md) | WITSML channels in HDF5 |
| [0014](0014-horizons-units-attachments-constraints.md) | Horizons and units, attachments, business constraints |
| [0015](0015-section-view.md) | The section view: three stages, `model.rs` as contract, one drawer |
| [0016](0016-one-crate-one-test-binary.md) | One crate, one test binary |
| [0017](0017-operation-is-a-pure-function.md) | An operation is a pure function; only `apply` writes |
| [0018](0018-synthetic-reference-model.md) | A synthetic reference model rather than an external corpus |
| [0019](0019-synthetic-data-is-flagged-and-sourced.md) | Synthetic data is flagged and sourced |
| [0020](0020-a-grid-is-never-reprojected.md) | A grid is never reprojected |
| [0021](0021-own-tool-trait-rmcp-as-adapter.md) | Our own `Tool` trait; the MCP SDK is an adapter behind a feature |
| [0022](0022-edge-kinds-from-the-specification.md) | The kind of an edge comes from the specification, labelled; the schema facts are generated |
| [0023](0023-objects-held-by-value.md) | The data objects held by value are listed apart, and write their own references |
| [0024](0024-facets-read-in-every-version.md) | Facets are read in every version, and say whether the schema lists them |
| [0025](0025-log-mapping-of-a-colour-map.md) | A logarithmic mapping places the log of the value |
| [0026](0026-a-head-portion-says-whether-the-file-describes-it.md) | A head portion says whether the file describes it, and is in the line's frame |
| [0027](0027-gaps-seams-scales-and-quantiles.md) | Gaps, seams, scales and quantiles: the numbers the views share |
| [0028](0028-one-drawing-for-every-view.md) | One drawing for every view: `draw` knows no format, one display list, one rule for graduations, a PNG is always the SVG rasterised |
| [0029](0029-convention-b-links-by-qualifier-facet.md) | A link to a table is one `qualifier` facet holding the uri of the table, read once; the axes of a property come from the table it names |
| [0030](0030-table-links-keys-and-ranks.md) | Table links: a column links by key or by rank, the target decides, one key column or none; description and title are read with a warning; an empty string is a value; the rules `house/table/*` |
| [0031](0031-tangent-at-a-knot.md) | The direction of a line at a knot is that of the interval that ends there |
| [0032](0032-hole-frame-in-a-direct-placed-frame.md) | The frame of a hole is the physical one, whatever hand the placed frame has |
| [0033](0033-tvd-below-the-md-datum.md) | A true vertical depth along a well is counted below the MD datum |
| [0034](0034-survey-north-and-stations.md) | The north of a survey and its stations: a north the file does not state is declared by the caller, the corrections the file states are applied, a rejected station is left out on declaration, a RESQML 2.0.1 deviation survey is read |
| [0035](0035-provenance-read-and-written.md) | Provenance, written completely and read from four releases: the role from the template then a named rule, `input` and `inputFile` declared, integers as integers, verbatim sources and the NWDF tables |
| [0036](0036-directional-ratio.md) | Two directional properties of one quantity keep the order a layered medium expects: a warning above 5 % of the pairs under 0.95, unverifiable on an extension direction unless the caller declares it |
| [0037](0037-results-are-artefacts-promoted-on-demand.md) | A result of a view is an artefact first and is stored on demand: one operation per sort of view, the drawing is the input and the recipe is read from it, an iso-surface promoted as it was computed |
| [0038](0038-geosteering-section.md) | The geosteering section: the curtain of a cube along a well, one frame and one door for placing, folds marked and gaps never bridged, declared conventions, angles found by value |
| [0039](0039-several-values-per-element-written.md) | Several values per element are written with their shape: `ValueCountPerIndexableElement` and `LabelPerComponent`, the dataset `[elements, d1, ...]`, a scalar written as it always was |
| [0040](0040-the-members-of-a-wellbore.md) | The members of a wellbore are walked in the domain, from the inventory and the references alone |
| [0041](0041-declared-conventions-and-the-vertical-without-crs.md) | A convention the file does not state is declared by the caller; a vertical without a CRS is signalled |
| [0042](0042-what-a-property-says-of-itself.md) | What a property says of itself: dimensions reduced together, scalars for a wrong declared count, facets of 2.0.1 outside the 2.3 list, the `synthetic` mark, observations beside those of the values |
| [0043](0043-the-views-of-a-package-through-one-door.md) | The views of a package are drawn through one door, as a display list written to SVG |
| [0044](0044-vectors-in-geostat.md) | The vector arithmetic and the small plan geometry live in `energyml_geostat::vector`; `energyml_geometry::vector` re-exports it |
| [0045](0045-text-and-numbers-one-copy.md) | Text and numbers written by hand (XML, JSON, base 64, calendar, numbers) have one copy each |
| [0046](0046-reading-a-field-through-one-door.md) | Reading a field of a reflected object goes through one door, `energyml_core::reflect::fields`, with its policy for absent, blank and mistyped fields |
| [0047](0047-a-view-proposes-and-says-so.md) | A view may propose a value the file does not state, for a first drawing; a proposal is always said as one |
| [0048](0048-every-edge-has-a-kind.md) | Every edge has a kind: EML and PRODML by the WITSML rule, primary by default otherwise |
| [0056](0056-one-door-from-a-value-to-a-colour.md) | One door from a value to a colour (`energyml_domain::color::ValueColours`): a constant, a discrete map read by exact key with the palette over the keys for the missing ones, a measure on the file's scale, a palette on the caller's extent, a missing value by `NullColor` |
| [0055](0055-the-unit-of-a-hue-is-read-from-the-colours.md) | The unit of a hue is read from the values of the set of colours it belongs to (`energyml_core::color::hue_unit`): fractions of a turn when all lie in 0-1 and one strictly between, degrees otherwise; said in a note and warned by the validation |
| [0054](0054-the-colours-along-a-well-come-through-one-door.md) | The colours along a borehole come through one door (`energyml_views::paint_wells`): a property chosen by its title on the frames of the well, carried by measured depth onto the nodes a section keeps, coloured by the package's cascade; several properties that fit are refused by name, a well without one is counted; a legend of categories beside the ramp; `energyml profile --paint-wells` |
| [0053](0053-contacts-read-through-the-limits-of-the-domain.md) | `contacts-from-frames` reads its runs through `energyml_domain::limits`: a class that comes back is no longer refused (ranked points, `which`), a categorical property and a threshold are read, a well the reading cannot treat is refused alone |
| [0052](0052-limits-across-a-well-and-the-ramp-legend-of-a-section.md) | The lines across a well at its limits are one thing with the strata ticks (one geometry, one style, drawn over a painted well too), and a section has one optional ramp legend that shares its arithmetic with the legend along a well |
| [0051](0051-the-limits-between-two-runs-of-values.md) | The limits between two runs of values are classified (units, codes, a table column, a threshold), found as runs and boundaries with a rank - a class that comes back is never refused - and placed in MD by an anchor |
| [0050](0050-the-colour-of-a-category-is-carried-by-its-table.md) | The colour of a category is carried by its table: a `ColorInformation` on the `CategoryLookup` is a rung of the cascade between the property and its kind, its map indexed by the table's keys, its `ValueVectorIndex` a column |
| [0049](0049-the-values-of-a-property-along-a-frame.md) | The values of a property along a wellbore frame are read once, by `frame_values`: one door for the intervals of a frame, the row of a table, the value at an MD, a table column as a property |

Decisions about the ETP protocol implementation are recorded in the
[`retp`](https://github.com/geosiris-technologies/retp) repository.
