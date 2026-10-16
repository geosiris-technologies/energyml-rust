# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-mcp-v0.2.0...energyml-mcp-v0.3.0) - 2026-10-16

### Added

- *(storage)* [**breaking**] the store times may be unknown
- *(storage)* list the data objects held by value, each the source of what it writes
- *(storage)* state the kind of the edges a store derives, from the specification

### Fixed

- *(deps)* move to zip 9, every 7.x and 8.x release being yanked

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/releases/tag/energyml-mcp-v0.2.0) - 2026-10-15

### Added

- *(mcp)* the stdio MCP server and the energyml-mcp binary
- *(mcp)* the tools of a generic read-only MCP server over the storage traits
