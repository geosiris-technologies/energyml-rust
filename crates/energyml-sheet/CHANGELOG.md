# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-sheet-v0.3.0...energyml-sheet-v0.4.0) - 2026-11-16

### Added

- *(sheet)* hit_within with a grab distance, and the outline of an element
- *(sheet,views)* text stays on the page and the sheet's headers are bold again
- *(sheet)* one PNG encoder for a raster in every build, so the SVG of a view with an image is written without the png feature
- *(sheet)* [**breaking**] the well sheet drawn through the display list and its one SVG writer; the well-sheet view
- *(sheet)* layered graph layout
- *(sheet)* rasters, and the PNG of an SVG
- *(sheet)* the display list, its SVG writer and hit testing
- *(sheet)* the common drawing module, its ticks and its label spreading

### Fixed

- the joins of the finishing lot
- *(geometry,section,views)* faces keep their outward winding, tube stations their nodes, and unreadable data is said
- *(views)* one decision of the curtain's scale (along_well::curtain_scale) for the legend and the view; the band of notes in the page's unit; the four views registered

### Other

- the links rustdoc refuses under -D warnings, and the dates two Energistics citations lacked
- an object or a colour map is read once per operation
- the export run and the h5 repair live in the library, the faces only call them; one reading of --h5
- *(sheet)* the drawing keeps its own XML escaping, unescaping and base64, pinned to the core's doors by a test
- *(sheet)* the drawing's hex writer names no format crate, even in a comment
- *(geostat)* the vector arithmetic and the plan geometry in one module, energyml_geostat::vector
- *(core)* one copy of each text helper - XML escaping, base64, the civil calendar, JSON quoting, plain and short numbers
- *(sheet)* the bridge between Rgba and the drawing's Colour lives outside draw/, which names no format crate
- *(colours)* one reader and one writer of a hex colour, Rgba::parse and Rgba::to_hex*
- format and clippy on the first wave, and three test fixes (float tolerance, a share without a rule, a doc comment)

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-sheet-v0.1.2...energyml-sheet-v0.2.0) - 2026-10-15

### Added

- *(storage)* [**breaking**] complete the storage interface for ETP stores and servers
