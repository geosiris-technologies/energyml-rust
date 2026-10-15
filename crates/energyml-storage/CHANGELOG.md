# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-storage-v0.1.2...energyml-storage-v0.2.0) - 2026-10-15

### Added

- *(mcp)* the tools of a generic read-only MCP server over the storage traits
- *(storage)* [**breaking**] references and edges are two facts, and a contract every store plays
- *(storage)* [**breaking**] complete the storage interface for ETP stores and servers

### Fixed

- *(storage)* answer the edges with either end among the objects found

### Other

- *(storage)* what a store's edges are not, and how a transaction ends when refused

## [0.1.2](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-storage-v0.1.1...energyml-storage-v0.1.2) - 2026-10-14

### Added

- *(python)* build the wheels with HDF5 compiled in
