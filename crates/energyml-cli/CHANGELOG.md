# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- *(cli)* `energyml profile --svg --paint-wells TITLE` paints the wells of the section along the property whose title contains `TITLE`, with its legend, and prints how many wells were painted, had no such property or were refused; the strata lines stay over the paint with `--strata-ticks`; ADR 0054

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-cli-v0.3.0...energyml-cli-v0.4.0) - 2026-11-16

### Added

- *(views)* name each view, let an offer say what it shows and what it proposes (part, proposed), and let target propose a first drawing
- *(views)* propose a first drawing for the curtain of along-well - assist, the links writer and reader, the middle lateral row, view draw --proposed
- *(cli)* energyml view - list, about, draw and redraw the views of a package, SVG and PNG on demand
- *(build)* [**breaking**] integer parameters written as integers, every canonical template declares input and inputFile, the file a step read, and a recipe drafted back from a workflow

### Fixed

- *(views)* the title of an axis table as a String; the CLI reference with view draw --proposed
- a text array is read as text, not as NaN; the section reads its wells' strata through one cache
- *(cli,python)* the faces refuse what they cannot answer instead of answering wrong
- *(cli)* say where the arrays came from when it was not the file the package names

### Other

- build's mesh takes the hexahedron faces from energyml_geometry, the CLI's terrain reads the lattice and its voids together, and the CLI names a target package once
- the export run and the h5 repair live in the library, the faces only call them; one reading of --h5
- *(core)* one copy of each text helper - XML escaping, base64, the civil calendar, JSON quoting, plain and short numbers
- *(cli)* a runnable file is a file, then executable on unix - the if guard clippy 1.100 refuses
- one reading of NullValue, and text arrays through the Python bridge, the MCP tool and the CLI

## [0.3.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-cli-v0.2.0...energyml-cli-v0.3.0) - 2026-10-16

### Other

- update Cargo.lock dependencies

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-cli-v0.1.2...energyml-cli-v0.2.0) - 2026-10-15

### Added

- *(mcp)* the stdio MCP server and the energyml-mcp binary

## [0.1.2](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-cli-v0.1.1...energyml-cli-v0.1.2) - 2026-10-14

### Other

- a dist profile with thin LTO, and HDF5 compiled into the CLI archives

## [0.1.1](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-cli-v0.1.0...energyml-cli-v0.1.1) - 2026-10-14

### Added

- *(cli)* fill surfaces and hide the notes of a section drawing
