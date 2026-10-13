# energyml-cache

What a package keeps of itself, layer by layer: the index of links and property sheets, the colour walks, and the listings of what is attached to an object. A face holds one `Cache` per open store; every layer is optional and lazy. There is a single implementation: `Cache::off()` runs the same code over a slot that is thrown away when the call returns, so a cached answer and an uncached one cannot differ. A layer is invalidated exactly when what it read changes.

Colours are decided in one place (`colours`) for every face. The crate compiles to wasm.

There are no feature flags.

## Documentation

- API reference: <https://docs.rs/energyml-cache>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [library guide](../../docs/library.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
