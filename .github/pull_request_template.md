## What and why

<!-- One subject per pull request. What changes, and the reason. -->

## Checklist

- [ ] I did **not** edit anything under `crates/generated/` (open an issue for generated-code defects).
- [ ] Formatting was run per crate (`cargo fmt -p <crate>`), never `cargo fmt --all`.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] Tests added or updated, and each new check can fail (I know what to break to make it red).
- [ ] New test files are declared as a `mod` in the crate's `tests/main.rs`.
- [ ] Documentation and comments are in English.
- [ ] Anything the code decides where the specification is silent is a named refusal or a documented rule, not a guess.
- [ ] An operation that implements a published algorithm has a `# References` section in its module doc (a citation, or `TODO(reference)` saying it is missing).
