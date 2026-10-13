# Contributing to energyml-rust

Thank you for helping. Bug reports, reproductions and pull requests are welcome. By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md). Security problems go through [SECURITY.md](SECURITY.md), never through a public issue.

Everything — code, comments, messages, documentation — is written in **English**.

## The rules that matter

1. **`crates/generated/` is never edited by hand.** It is written by an internal Geosiris generator from the Energistics schemas, and a CI check rejects any change to it that does not come from the regeneration bot. If you find a defect in generated code, **open an issue** describing it; do not send a pull request for it.
2. **Never run `cargo fmt --all`.** rustfmt would rewrite the generated crates. Format the hand-written crates one by one:
   `cargo fmt -p energyml-storage` (and so on, per crate name). Check formatting the same way: `cargo fmt -p <crate> -- --check`.
3. **One test binary per crate.** Tests live in `tests/main.rs` with one `mod` line per test file; a new test file that is not declared there does not run. A test that must own process state keeps its own `[[test]]`.
4. **A test must be able to fail.** Before you push a new check, ask what you would have to break to make it fail; if the answer is "nothing", break something on purpose to find out. A check that passes on an empty set proves nothing.
5. **Refuse by name rather than guess.** Where the specification is ambiguous the library returns a named refusal, not a plausible answer. A missing capability (for example HDF5 not compiled in) is an error that names the missing feature, never an empty result.
6. **Tests are grounded in the specification, not in whatever a real file happens to contain.** Cite the document, page and paragraph in a comment; do not make a test open a reference document at run time.
7. **One door per access.** Look an object up through the existing lookup, build identities through the existing identity type, decide colours through the colour module. A second private implementation is a defect.
8. **Faces hold no logic.** The CLI parses arguments and prints; the Python bridge converts. Anything that decides lives in a library crate first, and is bridged afterwards.
9. **`energyml-core` stays free of native dependencies.** WebAssembly is a hard requirement; HDF5 and similar live behind optional features in the storage layer.
10. **A message and the test that asserts it change in the same commit.** Rewording an error, a refusal or a diagnostic without the assertion that checks it leaves the suite red, or worse, silently stops checking the wording.
11. **Everything here is in English** — code, comments, messages, test names and docs. French appears only as data: values quoted from a file or a French standard, place and geological names.

## Building and testing

```sh
cargo test --workspace -j 4 -- --test-threads=4     # the full suite
cargo test --workspace some_test_name               # one test, across the workspace
cargo test --workspace --test tests crossaudit::    # one module of a crate's test binary
cargo clippy --workspace --all-targets -- -D warnings
```

Do not run `cargo test -p <crate>`: resolving features for one crate differs from the workspace and forces a full rebuild.

Some tests read reference packages written by other tools. They are not in the repository; fetch them (pinned, SHA-256 verified) with:

```sh
python scripts/fetch-fixtures.py        # into target/fixtures/
```

The HDF5 tests need the HDF5 library installed (for example `libhdf5-dev` on Debian/Ubuntu) and the `hdf5` feature.

## Pull requests

- Keep a pull request to one subject, with a description of what changes and why.
- Add or update tests and documentation with the code.
- CI must pass: formatting per crate, clippy with `-D warnings`, tests on Linux, macOS and Windows, wasm32 checks, docs without warnings, `cargo deny`.
- Commit messages: a short imperative summary, then the reason.

## Licence

By contributing you agree that your contribution is licensed under the [Apache License 2.0](LICENSE), as the rest of the project.
