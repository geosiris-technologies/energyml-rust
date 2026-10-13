# energyml-roundtrip

**Not published** (`publish = false`).

A validation tool: reads real EnergyML documents, writes each part back, and compares the two structurally (elements, namespaces, attributes, values), ignoring what carries no information. It reports anything lost. It is run by maintainers on EPC files and is not part of the test suite, which depends on no external file.

There are no feature flags.

## Documentation

- API reference: <https://docs.rs/energyml-roundtrip>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
