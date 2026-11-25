# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- *(domain)* `frame_values`: the values of a property along a wellbore frame, read once - support, values, the row of the category table each code names, the value at an MD (never across a hole), a table column as a property, and the values carried onto another representation of the wellbore by MD; ADR 0049
- *(domain)* `IndexableElement::is_frame_interval`, the one door for `intervals` and `intervals from datum`
- *(domain)* `table::Lookup::read_described` and `table::ColumnMeta`: a table read with what each column says of itself; `table::CODE`
- *(domain)* the colour of a category carried by its table: `Level::CategoryTable`, `Cascade::table_decision` and `Cascade::resolve_category`, `ColorMap::sample_key`; a `ColorInformation` on the `CategoryLookup` colours the units of a well, its `ValueVectorIndex` a column of the table, an index that cannot be placed refused by name (issue 35); ADR 0050
- *(domain)* `limits`: classify a suite along a frame (units, codes, a table column, a threshold with `value >= threshold` at or above), find its runs, boundaries and limits with their rank - a class that comes back is never refused - and place a limit in MD (on intervals the node between, on nodes inside, outside or interpolated, never over a hole); ADR 0051

### Fixed

- *(domain)* a frame whose properties say `intervals from datum` is tabulated one row per interval by `wellbore_table`, like `intervals`, instead of every one of its properties being refused (issue 34)
- *(domain)* a property of several patches, several values per element or `RealizationIndices` is refused by name by `wellbore_table` and `well_strata`

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-domain-v0.3.0...energyml-domain-v0.4.0) - 2026-11-16

### Added

- *(python)* repair the array files through the library; dependents and _data follow the version an address names
- *(domain)* the palettes a face offers when the file says no colour map, quoted from matplotlib and ColorBrewer
- *(build)* write a property of several values per element; combine, isosurface, table-link and along-well-view operations; stored views; ADR 0037 and 0039
- *(rules)* the provenance theme - an activity follows its template, checked; ADR 0035's rules
- *(rules)* the property theme and the directional ratio check; ADR 0036
- *(domain)* the members of a wellbore, one entry per well whatever it holds; ADR 0040
- *(domain)* the provenance graph - edges, activity tree, declared steps, carriers, collections, focus and timeline
- *(domain)* verbatim provenance from NWDF tables and its reconciliation; the provenance page and ADR 0035
- *(domain)* read activities and their templates, the role of each parameter and its conformance
- *(build)* [**breaking**] integer parameters written as integers, every canonical template declares input and inputFile, the file a step read, and a recipe drafted back from a workflow
- *(geometry)* [**breaking**] read a survey apart from placing it and diagnose every check; declared and stated north, rejected stations, the RESQML 2.0.1 deviation survey
- *(domain)* a window over a large table, a generic forest, the structure of tables, and measured-depth extents and coverage
- *(rules)* the house/table link rules, discrete values checked by rank
- *(domain)* column classes, link readers, resolve and check table links, object chips
- *(domain)* [**breaking**] column cells with absences and empty strings, a key index, and a sheet per table in the package index
- *(domain)* a property read in slabs for its description, and what a property says of itself
- *(domain)* the kinship of properties and the candidates of the views, read from the index
- *(domain)* convention B links, the axes of components, matrix planes and WITSML array channels
- *(domain)* the shape of a property without flattening, component picks and partial block reads
- *(domain,geometry)* [**breaking**] the axis order of a CRS, the direction along a path, and one placed path for trajectories and surveys
- *(domain)* interpolation and stepping along the measured depth, and the stats house rules
- *(domain)* [**breaking**] streaming statistics, quantiles, scales and three unit families

### Fixed

- a text array is read as text, not as NaN; the section reads its wells' strata through one cache
- *(domain,python)* Held::at takes a Named's uuid as it stands and only its version from the uri; the unreadable refusal of the Python session is tested on the function both questions go through
- *(domain)* arrays refuse what they cannot read exactly, and findings, versions and tables are told apart by full uri
- *(build)* media writers declare no data object, and the table imports declare the frame they write
- *(build)* the activities energyml-build writes follow their templates
- *(domain)* re-export TallyRefusal from stats; give two provenance-graph tests a feature whose uuid no activity has; format, clippy and test fixes of the fourth wave
- *(geometry)* give the eml 2.3 vertical CRS of the test fixtures the unit the schema requires; format, clippy and test fixes of the third wave
- *(domain)* read a keyed category table by its key in the wellbore table and the strata
- *(geometry)* refine the locator line until the MD read back is right; format, clippy and test fixes of the second wave; ADR index in order
- *(domain)* a logarithmic colour mapping takes the log of the value
- *(domain)* [**breaking**] read property and column facets, rows of several values and fractional lookup keys
- *(storage)* [**breaking**] read HDF5 strings as text, and refuse rather than empty a column

### Other

- the links rustdoc refuses under -D warnings, and the dates two Energistics citations lacked
- *(domain)* pass the cells, not a borrow of them
- *(domain,storage,build)* fewer copies of whole buffers
- an object or a colour map is read once per operation
- the clippy findings of wave B (an auto-deref, a slice from a reference, find on a dyn iterator, a comparison that says NaN)
- the export run and the h5 repair live in the library, the faces only call them; one reading of --h5
- *(core)* reading a field of a reflected object goes through one door, energyml_core::reflect::fields
- *(geostat)* the vector arithmetic and the plan geometry in one module, energyml_geostat::vector
- *(core)* one parser of an ETP uri, a content type and a qualified type; a reference and its object share one uri
- *(core)* one copy of each text helper - XML escaping, base64, the civil calendar, JSON quoting, plain and short numbers
- *(colours)* one reader and one writer of a hex colour, Rgba::parse and Rgba::to_hex*
- *(build)* along-well-view calls section-view's uri resolution, part names and attachment writer instead of copies; format, clippy and test fixes of the fifth wave
- the geosteering section, writing a table link, the members of a wellbore, iso-surfaces and combined properties
- format and clippy on the first wave, and three test fixes (float tolerance, a share without a rule, a doc comment)
- one reading of NullValue, and text arrays through the Python bridge, the MCP tool and the CLI

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-domain-v0.1.2...energyml-domain-v0.2.0) - 2026-10-15

### Added

- *(storage)* [**breaking**] complete the storage interface for ETP stores and servers
