# energyml-derive

Procedural macros that turn the structs emitted by the code generator into implementations of the `energyml-core` traits. Three derives cover the whole model:

- `EnergymlObject` for an XSD complex type;
- `EnergymlEnum` for an enumeration (open ones also accept free text);
- `EnergymlChoice` for an abstract type or substitution group, selected on the wire by `xsi:type`.

Behaviour lives in the macros rather than in the generated crates, so a change to how a construct is serialised is a change to one file here and not a regeneration of every generated crate.

You normally do not depend on this crate directly: `energyml-core` re-exports it behind its default `derive` feature.

There are no feature flags.

## Documentation

- API reference: <https://docs.rs/energyml-derive>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [architecture](../../ARCHITECTURE.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
