# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- *(views)* the image track offers a property on `intervals from datum` like one on `intervals`

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-views-v0.3.0...energyml-views-v0.4.0) - 2026-11-16

### Added

- *(views)* name each view, let an offer say what it shows and what it proposes (part, proposed), and let target propose a first drawing
- *(views)* propose a first drawing for the curtain of along-well - assist, the links writer and reader, the middle lateral row, view draw --proposed
- *(views)* write_svg, the one writer of a view's SVG from a list already drawn, and target::frame_mds
- *(sheet,views)* text stays on the page and the sheet's headers are bold again
- *(views)* register the property-insight view; one reading of a component pick for every view
- *(views)* property-insight, what a property holds - its form, distribution, relation with a sibling and variation along each axis
- *(views)* survey-qc, the control of a directional survey in panels
- *(views)* md-coverage, which objects cover which measured depths of a well, a band per MD reference
- *(views)* table-curve, the curves of a table with a legend per column and a second axis for a second family of units
- *(sheet)* one PNG encoder for a raster in every build, so the SVG of a view with an image is written without the png feature
- *(views)* provenance-graph and table-schema, two graphs laid in layers through one graph module
- *(views)* target, the hole seen end on at one measured depth, a property in rings and sectors under a declared angle convention
- *(views)* image-track, a property of several values per element as an image along the measured depth
- *(sheet)* [**breaking**] the well sheet drawn through the display list and its one SVG writer; the well-sheet view
- *(views)* along-well, the geosteering section as a view of the registry
- *(views)* energyml-views, the views of a package as pictures through one door, and the well locator; ADR 0043

### Fixed

- *(views)* an offer proposes the links of a curtain whose only fitting table keeps its columns in an external array
- *(views)* the title of an axis table as a String; the CLI reference with view draw --proposed
- the joins of the finishing lot
- *(views)* docs.rs builds the png feature only, since it has no native HDF5
- *(export,views)* a dyn iterator is walked by a loop; the target's frame address does not shadow its support
- *(geometry,section,views)* faces keep their outward winding, tube stations their nodes, and unreadable data is said
- *(views)* the PNG answers by the png feature of energyml-views, not by the sheet's
- *(views)* the three views of the second wave registered
- *(views)* the two graph views registered; clippy
- *(views)* one decision of the curtain's scale (along_well::curtain_scale) for the legend and the view; the band of notes in the page's unit; the four views registered

### Other

- *(views)* link the function assist, not its module
- *(views)* rustfmt of the names and offers
- the links rustdoc refuses under -D warnings, and the dates two Energistics citations lacked
- an object or a colour map is read once per operation
- the export run and the h5 repair live in the library, the faces only call them; one reading of --h5
- *(geostat)* the vector arithmetic and the plan geometry in one module, energyml_geostat::vector
- *(core)* one copy of each text helper - XML escaping, base64, the civil calendar, JSON quoting, plain and short numbers
- *(colours)* one reader and one writer of a hex colour, Rgba::parse and Rgba::to_hex*
- *(views)* depend on energyml-analysis for the survey quality control view
- *(views)* the along-well projection drawn is the one asked, told by the abscissa's graduations
