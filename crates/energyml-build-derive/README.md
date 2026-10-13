# energyml-build-derive

Derive macros for `energyml-build`. An operation's parameters, choices and inputs are declared once, as Rust types; the form, the reading and the activity template all come from that declaration.

- `Choice`: a unit enum becomes a closed list of words;
- `Params`: a struct becomes one slot per field (nested with `flatten`), an enum with a tag becomes a union of cases;
- `Inputs`: one input slot per field, bound by the framework.

You do not depend on this crate directly; use `energyml-build`.

There are no feature flags.

## Documentation

- API reference: <https://docs.rs/energyml-build-derive>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [creating objects](../../docs/creating-objects.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
