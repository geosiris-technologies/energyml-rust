# energyml-cli

The `energyml` command, and its short alias `enyml`: inspect, validate, export, repair and create EnergyML packages from a terminal. The CLI parses arguments and prints; every check and operation lives in the libraries. It is built with every schema version (`energyml-schemas/all`); the per-version features belong to the `energyml` library crate.

```sh
cargo install energyml-cli        # the package installs the `energyml` binary
energyml list model.epc
energyml check model.epc          # exit status 1 on errors
energyml arrays --summary model.epc
energyml show --json model.epc <uuid>
energyml create --list --from export.geojson
```

The CLI reference documents every subcommand and option.

## Features

| Feature | Default | Effect |
|---|---|---|
| `hdf5` | no | read and write arrays (forwards to storage, geometry, export, build); needs the HDF5 C library at build time |
| `png` | no | `profile --png` rasterises the section view |
| `private-corpus` | no | test-only: runs the suites that need real files kept outside the repository |

Commands of other repositories run as external subcommands: `energyml etp …` runs `retp` (install `retp-cli`), and any other `energyml <word> …` runs `energyml-<word>` if it is on the `PATH`. `energyml --help` lists the ones it finds.

## Documentation

- API reference: <https://docs.rs/energyml-cli>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md), [CLI guide](../../docs/cli.md), [CLI reference](../../docs/cli-reference.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
