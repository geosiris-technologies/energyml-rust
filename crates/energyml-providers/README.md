# energyml-providers

Queries public geoscience APIs and reads what they answer. Building a request and reading a response are pure functions and are tested without a network; the round trip is a `Transport` trait. A `Provider` declares its collections and, for each, the filters it accepts (type, whether required, suggested value), so a form can be rendered without naming a single field. Query-axis order depends on the service and is handled per provider. The quote (what would be fetched) comes before the fetch.

The data sources document lists each provider with its licence and attribution.

## Features

| Feature | Default | Effect |
|---|---|---|
| `native` | no | a native HTTP transport (`ureq`), streaming the body; without it only the pure half is available (for example in a browser, where the host supplies `fetch`) |

## Documentation

- API reference: <https://docs.rs/energyml-providers>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [data sources](../../docs/data-sources.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
