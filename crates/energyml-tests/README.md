# energyml-tests

**Not published** (`publish = false`).

End-to-end tests over the generated crates and the libraries above them, plus the cross-audit: checks that confront two independent sources of the same fact (the bytes of an HDF5 file, the part names of a zip, the counts an XML document declares) so that two readers cannot agree on a wrong total. All tests are compiled into a single binary, `tests/main.rs`; a new test file is a `mod` line there.

## Features

| Feature | Default | Effect |
|---|---|---|
| `hdf5` | no | enables array reading in storage, geometry and domain for the tests that need it |

## Documentation

- API reference: <https://docs.rs/energyml-tests>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
