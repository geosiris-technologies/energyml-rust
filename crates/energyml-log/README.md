# energyml-log

The one `tracing` subscriber a face installs. Libraries in this workspace emit spans and events and never configure anything; the command line (or any embedding program) calls `init` or `init_reloadable` once.

- **Off by default**: with no filter argument and no `ENERGYML_LOG` variable, nothing is installed.
- Filter syntax is `tracing_subscriber::EnvFilter` (`info`, `energyml_build=debug,energyml_storage=info`); `ENERGYML_LOG_FORMAT` selects `text` or `json`.
- Output always goes to stderr, so stdout stays clean for results.
- A closing span reports `time.busy` and `time.idle`.
- With `init_reloadable`, the filter can be changed while the program runs.
- `log` crate records are bridged to the same filter.

```rust
let handle = energyml_log::init(None, None)?; // honours ENERGYML_LOG
```

There are no feature flags.

## Documentation

- API reference: <https://docs.rs/energyml-log>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [logging](../../docs/logging.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
