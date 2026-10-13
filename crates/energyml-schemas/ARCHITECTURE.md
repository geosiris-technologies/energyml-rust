# energyml-schemas: architecture

## Purpose and position

One `Registry` holding every generated schema version. `energyml-storage` takes a registry and never builds one, so that "a new schema version costs no code change in the storage layer" stays true; somebody still has to choose which versions an application knows, and for the tools shipped here the answer is all of them. This crate is that single place. It depends on `energyml-core` and, optionally, on every generated crate; every tool and test above it uses `energyml_schemas::all()`.

Related decisions: [ADR-0001](../../docs/adr/0001-generated-code-is-committed.md), [ADR-0004](../../docs/adr/0004-reading-policy.md), [ADR-0006](../../docs/adr/0006-registries-rather-than-match.md).

## Rules

**The list of releases exists once** — it used to be written out separately in each tool and in each test suite, every copy an independently maintained subset, and nothing detected an omission: a new crate left out of one list produced neither a compile error nor a failing test, only a tool that quietly could not read the new version. A test (`tests/complete.rs`) checks the list against the generator configuration, so adding a release and forgetting the line fails by naming it.

**A new release costs one optional dependency, one feature, one `KNOWN_VERSIONS` row and one `register` line here** — identification is driven entirely by `TypeInfo`: content types, qualified types, part names and version resolution are computed from the registry, not matched against a table, and the geometry readers are keyed on the concept name, identical across versions. What does cost something is a release that respells the inside of a type; those spellings live in named constants next to the code that reads them, not here.

**Everything is read; only releases are written** — `all()` registers every version, development drops included, because a file written against a drop must still open. What a new object may be made of is narrower, and `may_create` draws that line once so every face offering a type to create (a page, an operation, a language binding) offers the same ones. A drop-only type would write objects no released reader knows, and an offered drop shows the same type name twice.

**One feature per schema version, named as a qualified type spells it** (`eml23`, `resqml20`, `resqml22`, `resqml22dev3`, `witsml21`, `prodml23`…), plus the families and `all`. Each generated crate is an optional dependency; a version feature enables its crate and the EML version that crate is built on (`resqml22` brings `eml23`, `resqml22dev3` brings `eml22`), so an application can take RESQML 2.2 without 2.0.1, and EML versions stay features of their own. `default = ["all"]` keeps the faces that never chose working; the `energyml` facade has no default version at all. `all()` registers only what is compiled.

**A version that is not compiled is a named refusal, never a half read** — `KNOWN_VERSIONS` is a table of every version the project ships, deliberately not feature-gated, so that an empty lookup can be told apart from a version nobody knows: a known version whose feature is off yields `MissingFeature` ("RESQML 2.2 is not compiled in: enable the `resqml22` feature…"), an unknown one stays unknown. Resolution itself is untouched: `2.0`, `2.0.1`, `resqml20.X` and `resqml201.X` are one row, `2.2` is the release and `2.2_dev3` the drop. The refusal takes the set of compiled features as a parameter (`refusal`), which is how it is tested in a workspace build that has every feature on.

**Registration order is deliberate, and not load-bearing alone** — a pre-release drop is registered before the release it precedes, and `Registry::register` also refuses to let a drop displace a release, so neither the line nor its position can re-point release lookups at the drop.

**The registered pairs are derived, not written** — the list of `(package, version)` pairs is read back from the registry, so it cannot disagree with what `all()` actually did, which is the point of the test that consumes it.

## Deliberately not here

- No version-specific logic beyond the registration list and `may_create`.
- No storage, no reading, no policy: the reading policy is in `energyml-core`.

## How it is tested

`tests/features.rs` holds the table against `crates/generated/` and `Cargo.toml` (every generated crate has one row and one feature; the feature enables its crate and its EML; the families cover every version) and tests the refusal with the compiled set given. `tests/complete.rs` confronts the registry with the generated crates, and holds that a drop is read but never offered for creation and that every creatable type is what its qualified type creates. Each check names what it would take to break it.
