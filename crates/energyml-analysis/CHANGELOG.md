# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-analysis-v0.3.0...energyml-analysis-v0.4.0) - 2026-11-16

### Added

- *(analysis)* combine two properties cell by cell - ratio, log ratio, difference
- *(geostat)* an iso-surface of a lattice by marching tetrahedra
- *(analysis)* survey_qc, azimuth significance, section azimuth, comparison, stated values and pairings

### Fixed

- *(analysis,geostat,export)* local kriging, folded faces, outline simplification and the exported formats say what is true
- *(domain)* re-export TallyRefusal from stats; give two provenance-graph tests a feature whose uuid no activity has; format, clippy and test fixes of the fourth wave
- *(geometry)* give the eml 2.3 vertical CRS of the test fixtures the unit the schema requires; format, clippy and test fixes of the third wave

### Other

- one bucket grid in geostat replaces the full scans of the fault, section, fence and grid code
- *(analysis)* one reading of a fault trace and one hanging-wall rule; one table of hexahedron faces; one sample check for the interpolators
- *(geostat)* the vector arithmetic and the plan geometry in one module, energyml_geostat::vector
- the geosteering section, writing a table link, the members of a wellbore, iso-surfaces and combined properties

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-analysis-v0.1.2...energyml-analysis-v0.2.0) - 2026-10-15

### Other

- released with the rest of the workspace at 0.2.0
