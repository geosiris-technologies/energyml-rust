# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-geometry-v0.3.0...energyml-geometry-v0.4.0) - 2026-11-16

### Added

- *(geometry)* [**breaking**] read a survey apart from placing it and diagnose every check; declared and stated north, rejected stations, the RESQML 2.0.1 deviation survey
- *(geometry)* around the well: the hole frame, polar layers, transverse grids, envelopes and 2D projections
- *(domain,geometry)* [**breaking**] the axis order of a CRS, the direction along a path, and one placed path for trajectories and surveys

### Fixed

- a text array is read as text, not as NaN; the section reads its wells' strata through one cache
- *(geometry,section,views)* faces keep their outward winding, tube stations their nodes, and unreadable data is said
- *(geometry)* two triangles put their shared edge's cut on the same point, so a cut through a real mesh welds into one polyline
- *(geometry)* give the eml 2.3 vertical CRS of the test fixtures the unit the schema requires; format, clippy and test fixes of the third wave
- *(geometry)* refine the locator line until the MD read back is right; format, clippy and test fixes of the second wave; ADR index in order
- *(geometry)* [**breaking**] place the head portion in the line's own frame, one vector module, say a missing CRS

### Other

- the links rustdoc refuses under -D warnings, and the dates two Energistics citations lacked
- one bucket grid in geostat replaces the full scans of the fault, section, fence and grid code
- *(analysis)* one reading of a fault trace and one hanging-wall rule; one table of hexahedron faces; one sample check for the interpolators
- *(core)* reading a field of a reflected object goes through one door, energyml_core::reflect::fields
- *(geostat)* the vector arithmetic and the plan geometry in one module, energyml_geostat::vector
- *(core)* one parser of an ETP uri, a content type and a qualified type; a reference and its object share one uri
- *(core)* one copy of each text helper - XML escaping, base64, the civil calendar, JSON quoting, plain and short numbers
- format and clippy on the first wave, and three test fixes (float tolerance, a share without a rule, a doc comment)

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-geometry-v0.1.2...energyml-geometry-v0.2.0) - 2026-10-15

### Added

- *(storage)* [**breaking**] complete the storage interface for ETP stores and servers
