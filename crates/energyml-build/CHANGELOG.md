# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- *(build)* `contacts-from-frames` reads a categorical property (`categoryKind`, `codes`, `labels` - the classes are titled with their label) or a threshold on a continuous one (`thresholdKind`, `threshold`, `thresholdSide`) as well as the units, places a contact on nodes `inside`, `outside` or `interpolated` (`place`; the interpolated point on the frame's trajectory at its measured depth, a frame without a readable trajectory refused alone), and keeps all, the first or the last contact of each class and side in each hole (`which`); each point carries its rank in a `rank` property of its point set, written when a class comes back; ADR 0053

- *(build)* `contacts-from-frames`, interpolated, places a contact of a frame whose trajectory has no `Geometry` on the vertical read from its datum, as `well::frame` places the frame's nodes, and says so (`contacts-on-datum-vertical`); `KnownPath` gains `along`, the placement `well::frame` draws with, beside `path`, the strict line

### Changed

- *(build)* [**breaking**] `contacts-from-frames` no longer refuses a unit that comes back lower in the same hole (A, B, A): it gives two ranked tops and bases in the same point sets, and the run goes on; a frame the reading cannot treat is refused alone, named in the plan, and the others are read. Its runs, ranks and places are those of `energyml_domain::limits`; a run with no repeated crossing writes what it wrote before

### Fixed

- *(build)* the templates of `transform`, `densify`, `delete-nodes`, `loft`, `polyline-through`, `estimate` and `resurface` declare the parameters their activities record (the counts of the shape written by the shared writers, through one helper in the operations' support module, and the fit's `method`, `levels`, `sampleCount`, `fitSampleCount`, `allDimensionsAreOrthogonal`); found by running every operation and passing the conformance rule on what it wrote
- *(build)* `apply` replaces a template of the operation that the package already holds when its content differs from the canonical one (title, allowed kinds, `MinOccurs`, `MaxOccurs` of each parameter), and says so in the new `Made::superseded`; a template of the same content is left alone, and an object that is not a template under the template's uuid is refused by name. Before, an older template was kept and every parameter it lacked read as a defect of the new activities. `Made` gains a public field.

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-build-v0.3.0...energyml-build-v0.4.0) - 2026-11-16

### Added

- *(build)* write a property of several values per element; combine, isosurface, table-link and along-well-view operations; stored views; ADR 0037 and 0039
- *(build)* [**breaking**] integer parameters written as integers, every canonical template declares input and inputFile, the file a step read, and a recipe drafted back from a workflow
- *(domain,geometry)* [**breaking**] the axis order of a CRS, the direction along a path, and one placed path for trajectories and surveys

### Fixed

- a grid column is judged where the path meets it, bytes posed on an object part replace it in the save, a negative count of cuts is refused
- *(build)* a GeoJSON feature the reader refuses is left out and named in Read::warnings, not a refusal of the file
- *(build)* the operations refuse by name what they used to drop, guess or overflow
- *(build)* media writers declare no data object, and the table imports declare the frame they write
- *(build)* the activities energyml-build writes follow their templates
- *(domain)* re-export TallyRefusal from stats; give two provenance-graph tests a feature whose uuid no activity has; format, clippy and test fixes of the fourth wave

### Other

- the clippy findings of the finishing lot (a named type for the versions held in memory, let-else in two tests)
- *(build)* [**breaking**] share the geometry, features and table of an input behind an Arc
- the links rustdoc refuses under -D warnings, and the dates two Energistics citations lacked
- one bucket grid in geostat replaces the full scans of the fault, section, fence and grid code
- *(build)* reproject a whole array with one call to reproject_all instead of one call per point
- *(domain,storage,build)* fewer copies of whole buffers
- the clippy findings of wave B (an auto-deref, a slice from a reference, find on a dyn iterator, a comparison that says NaN)
- *(build)* pass the citation function, not a borrow of it
- build's mesh takes the hexahedron faces from energyml_geometry, the CLI's terrain reads the lattice and its voids together, and the CLI names a target package once
- *(build)* one copy of each block the operations shared
- *(core)* reading a field of a reflected object goes through one door, energyml_core::reflect::fields
- *(geostat)* the vector arithmetic and the plan geometry in one module, energyml_geostat::vector
- *(core)* one parser of an ETP uri, a content type and a qualified type; a reference and its object share one uri
- *(core)* one copy of each text helper - XML escaping, base64, the civil calendar, JSON quoting, plain and short numbers
- *(colours)* one reader and one writer of a hex colour, Rgba::parse and Rgba::to_hex*
- *(build)* along-well-view calls section-view's uri resolution, part names and attachment writer instead of copies; format, clippy and test fixes of the fifth wave
- one reading of NullValue, and text arrays through the Python bridge, the MCP tool and the CLI

## [0.3.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-build-v0.2.0...energyml-build-v0.3.0) - 2026-10-16

### Fixed

- *(deps)* move to zip 9, every 7.x and 8.x release being yanked

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-build-v0.1.2...energyml-build-v0.2.0) - 2026-10-15

### Added

- *(storage)* [**breaking**] complete the storage interface for ETP stores and servers
