# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-export-v0.3.0...energyml-export-v0.4.0) - 2026-11-16

### Fixed

- *(export,views)* a dyn iterator is walked by a loop; the target's frame address does not shadow its support
- *(analysis,geostat,export)* local kriging, folded faces, outline simplification and the exported formats say what is true

### Other

- the clippy findings of wave B (an auto-deref, a slice from a reference, find on a dyn iterator, a comparison that says NaN)
- the export run and the h5 repair live in the library, the faces only call them; one reading of --h5
- *(analysis)* one reading of a fault trace and one hanging-wall rule; one table of hexahedron faces; one sample check for the interpolators
- *(core)* one copy of each text helper - XML escaping, base64, the civil calendar, JSON quoting, plain and short numbers

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-export-v0.1.2...energyml-export-v0.2.0) - 2026-10-15

### Other

- released with the rest of the workspace at 0.2.0
