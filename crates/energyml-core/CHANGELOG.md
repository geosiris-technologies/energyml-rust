# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- *(core)* `color::hue_unit`, `color::to_degrees` and `color::HueUnit`: the unit of the hues of a set of colours, read from the numbers (ADR 0055)

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-core-v0.3.0...energyml-core-v0.4.0) - 2026-11-16

### Added

- *(python)* repair the array files through the library; dependents and _data follow the version an address names

### Fixed

- *(core,storage)* no write before the check, no part lost, and a transaction rolls everything back

### Other

- the clippy findings of the finishing lot (a named type for the versions held in memory, let-else in two tests)
- *(core,geostat)* a doc sentence clippy does not read as a list, and the outline deviation filled by an iterator
- *(core)* the dataspace quote test cites the ETP regular expressions it follows
- *(core)* document the test types of the field door
- *(core)* reading a field of a reflected object goes through one door, energyml_core::reflect::fields
- *(core)* one parser of an ETP uri, a content type and a qualified type; a reference and its object share one uri
- *(core)* one copy of each text helper - XML escaping, base64, the civil calendar, JSON quoting, plain and short numbers
- settle the TODOs that the code, the CLI and the client's docs answer

## [0.3.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-core-v0.2.0...energyml-core-v0.3.0) - 2026-10-16

### Added

- *(storage)* open an EPC as a named dataspace
- *(storage)* warn of an object whose uuid is the nil UUID
- *(core)* references grouped by the data object that writes them
- *(core)* where a type holds data objects by value, and where it refers to one

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-core-v0.1.2...energyml-core-v0.2.0) - 2026-10-15

### Added

- *(mcp)* the tools of a generic read-only MCP server over the storage traits
