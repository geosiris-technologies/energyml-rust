# energyml-core

Runtime support for the generated EnergyML crates (EML, RESQML, WITSML, PRODML). It holds everything generated code leans on and nothing schema-specific: namespace-aware XML reading and writing, JSON through serde, a static reflection table per type, dynamic access by path, structural validation, a registry that creates and reads objects by content type or qualified type, and the parsers for UUIDs, content types, qualified types and ETP URIs.

The crate has **no native dependency**, so everything built on it up to the geometry layer compiles for `wasm32`.

## Features

| Feature | Default | Effect |
|---|---|---|
| `json` | yes | JSON (de)serialisation through `serde_json` |
| `derive` | yes | re-exports the derive macros of `energyml-derive` |

## Example

Identifiers are parsed in one place, `energyml_core::identifiers`; `EtpUri::parse` is the single entry point for ETP URIs.

```rust
use energyml_core::identifiers::EtpUri;

let uri = EtpUri::parse(text)?; // `text`: an ETP URI string
```

For instance `eml:///resqml22.WellboreFrameRepresentation(0c02d234-e357-5361-812e-00a03be32b0a)`,
or, in a named dataspace, `eml:///dataspace('demo')/resqml22.TriangulatedSetRepresentation(<uuid>)`.

## Documentation

- API reference: <https://docs.rs/energyml-core>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [library guide](../../docs/library.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
