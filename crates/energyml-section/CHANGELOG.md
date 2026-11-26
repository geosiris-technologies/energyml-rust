# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- *(section)* a stretch of a painted well that the paint has no value for is drawn as an unpainted well is (the view's colour, or its strata with `strata_for_unpainted`) instead of being left empty; `paint::gaps` names the stretches
- *(section)* `View::strata_for_unpainted`: under scene colouring the wells with no paint keep their strata (intervals, ticks, strata legend); off by default and not written when off; ADR 0054
- *(section)* the wells that carry a paint are drawn after the others of the section, so a frame drawn in its own colour does not hide the painted one
- *(section)* a legend of categories, one per section (`View::category_legend`, `CategoryLegend`), under the other legends on the right, in a group of its own (`category-legend`); `limits::strata_limits`, the lines between the strata units a well shows as limits (`energyml_domain::limits`), drawn over any colour the well has; ADR 0054

- *(section)* limits across a well (`View::limits`, `limit_style`, `limits::well_limits` from `energyml_domain::limits`), drawn over its strata and over a paint the scene lays along it; the strata ticks are one case of them and take the same colour and width (the default is the black line of one pixel they always were); a ramp legend, one per section (`View::ramp_legend`, `ramp::sampled`), whose steps and graduations are those of the legend along a well; ADR 0052

### Fixed

- *(section)* the wells of a section are folded by the pair (package, wellbore), not by the wellbore alone: two packages that hold the same wellbore under the same uuid are two lines, each from its own package, where the second used to be folded into the first's line and its paint lost
- *(section)* the comment of `cut_sources` said `TraceItem` carries no `sources`; it does

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-section-v0.3.0...energyml-section-v0.4.0) - 2026-11-16

### Added

- *(sheet,views)* text stays on the page and the sheet's headers are bold again
- *(sheet)* one PNG encoder for a raster in every build, so the SVG of a view with an image is written without the png feature
- *(section)* the section along a well - curtain of a matrix property, boundaries, horizons, markers, layout and pick; ADR 0038
- *(sheet)* the common drawing module, its ticks and its label spreading

### Fixed

- a text array is read as text, not as NaN; the section reads its wells' strata through one cache
- *(geometry,section,views)* faces keep their outward winding, tube stations their nodes, and unreadable data is said
- *(views)* one decision of the curtain's scale (along_well::curtain_scale) for the legend and the view; the band of notes in the page's unit; the four views registered
- *(domain)* re-export TallyRefusal from stats; give two provenance-graph tests a feature whose uuid no activity has; format, clippy and test fixes of the fourth wave
- *(geometry)* give the eml 2.3 vertical CRS of the test fixtures the unit the schema requires; format, clippy and test fixes of the third wave

### Other

- the links rustdoc refuses under -D warnings, and the dates two Energistics citations lacked
- one bucket grid in geostat replaces the full scans of the fault, section, fence and grid code
- the export run and the h5 repair live in the library, the faces only call them; one reading of --h5
- *(geostat)* the vector arithmetic and the plan geometry in one module, energyml_geostat::vector
- *(core)* one copy of each text helper - XML escaping, base64, the civil calendar, JSON quoting, plain and short numbers
- *(colours)* one reader and one writer of a hex colour, Rgba::parse and Rgba::to_hex*
- the geosteering section, writing a table link, the members of a wellbore, iso-surfaces and combined properties

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-section-v0.1.2...energyml-section-v0.2.0) - 2026-10-15

### Added

- *(storage)* [**breaking**] complete the storage interface for ETP stores and servers
