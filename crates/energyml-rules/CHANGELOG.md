# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-rules-v0.3.0...energyml-rules-v0.4.0) - 2026-11-16

### Added

- *(rules)* the provenance theme - an activity follows its template, checked; ADR 0035's rules
- *(rules)* the property theme and the directional ratio check; ADR 0036
- *(rules)* the survey theme - declared north, north corrections, rejected stations, deviation surveys
- *(rules)* the house/table link rules, discrete values checked by rank
- *(domain)* interpolation and stepping along the measured depth, and the stats house rules

### Fixed

- *(domain)* a logarithmic colour mapping takes the log of the value

### Other

- *(build)* along-well-view calls section-view's uri resolution, part names and attachment writer instead of copies; format, clippy and test fixes of the fifth wave
- *(rules)* the well theme, its four house rules, ADR 0031 to 0033 and 0041, and the around-the-well page
- format and clippy on the first wave, and three test fixes (float tolerance, a share without a rule, a doc comment)

## [0.2.0](https://github.com/geosiris-technologies/energyml-rust/compare/energyml-rules-v0.1.2...energyml-rules-v0.2.0) - 2026-10-15

### Other

- released with the rest of the workspace at 0.2.0
