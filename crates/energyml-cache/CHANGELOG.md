# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- *(cache)* a category property is painted by the `ColorInformation` on its table before its kind and above, in the scene's `Cache::decision` as in `of_package` and `decisions_of` (issue 35)

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-cache-v0.3.0...energyml-cache-v0.4.0) - 2026-11-16

### Fixed

- *(cache)* the colour decisions take each summary by reference

### Other

- an object or a colour map is read once per operation
- *(colours)* one reader and one writer of a hex colour, Rgba::parse and Rgba::to_hex*

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-cache-v0.1.2...energyml-cache-v0.2.0) - 2026-10-15

### Other

- released with the rest of the workspace at 0.2.0
