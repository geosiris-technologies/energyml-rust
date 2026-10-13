# energyml-schemas

Every EnergyML schema version this workspace ships, gathered in one `Registry`. The storage layer takes a registry and never builds one; this crate is where an application that wants "all the versions" gets it, so the list of releases exists in exactly one place. A test checks the list against the generator configuration, so a new release that is not registered fails the build instead of silently going unread.

## Choosing versions

One Cargo feature per schema version, spelled as Energistics spells a qualified type. By default (`default = ["all"]`) every version is compiled; a library that reads fewer says so:

```toml
# RESQML 2.2 and the EML 2.3 it is built on, nothing else
energyml-schemas = { version = "0.1", default-features = false, features = ["resqml22"] }
```

| Feature | Compiles | Also enables |
|---|---|---|
| `eml20` `eml21` `eml22` `eml23` | EML 2.0 to 2.3 | |
| `resqml20` | RESQML 2.0.1 | `eml20` |
| `resqml22` | RESQML 2.2 | `eml23` |
| `resqml22dev3` | RESQML 2.2_dev3, the development drop | `eml22` |
| `witsml20` `witsml21` | WITSML 2.0, 2.1 | `eml21`, `eml23` |
| `prodml20` `prodml22` `prodml23` | PRODML 2.0, 2.2, 2.3 | `eml21`, `eml23`, `eml23` |
| `eml` `resqml` `witsml` `prodml` | every version of one family | |
| `all` | every version (the default) | |

`2.0`, `2.0.1`, `resqml20.` and `resqml201.` all name the 2.0.1 crate, so the `1` is not needed to read it. A version that is not compiled is a **named refusal**, never a half read: `energyml_schemas::missing_feature("resqml", "2.2")` answers "RESQML 2.2 is not compiled in: enable the `resqml22` feature of energyml-schemas (or of energyml)", and `KNOWN_VERSIONS` lists every version the project ships, compiled or not.

## Example

```rust
let registry = std::sync::Arc::new(energyml_schemas::all());
```

`all()` registers every generated crate compiled in and returns the `energyml_core::registry::Registry` to hand to `energyml-storage`.

## Documentation

- API reference: <https://docs.rs/energyml-schemas>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [supported schema versions](../../docs/schemas.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
