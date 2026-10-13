# energyml-rules

The house rules: the decisions this library makes where the specification is silent, ambiguous or only interpreted. Each rule is a registry entry with an id, an explanation and references (a page and paragraph of a specification, or a design decision). The rules are TOML files compiled by `build.rs` into statics, so a misspelt id does not compile and nothing is parsed at run time. The crate has no runtime dependency, wasm included, and any crate can cite a rule by id while a face can list them all.

## Features

| Feature | Default | Effect |
|---|---|---|
| `fr` | yes | compile in the French translations; English is the source and is always present, and a rule without a translation answers in English and says so |

## Documentation

- API reference: <https://docs.rs/energyml-rules>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [house rules](../../docs/house-rules.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
