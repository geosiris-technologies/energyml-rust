# Releasing

How the crates and the Python wheels of this repository are published. Nothing
is published from a workstation once the first release is out: publication runs
from the CI of this repository, on its default branch, with credentials that
only protected workflows can read.

## What is published where

| What | Where | Triggered by |
|---|---|---|
| the library crates (see the order below) | crates.io | `release-plz` ([`release-plz.toml`](../release-plz.toml), `.github/workflows/release-plz.yml`) after a release pull request is merged |
| `energyml` and `enyml` binaries | GitHub releases | tag `energyml-cli-vX.Y.Z` (`release-cli.yml`) |
| the `energyml` image | `ghcr.io/geosiris-technologies/energyml` | `main` (tag `edge`) and the same tag (`docker.yml`) |
| `energyml-rust` wheels (imported as `energyml.rust`) | PyPI | tag `python-vX.Y.Z` (`python.yml`) |
| not published | | `energyml-tests`, `energyml-roundtrip`, `energyml-python` as a crate (it ships as wheels) |

## Publication order

A crate can be published only when everything it depends on is already on
crates.io. The order below comes from the manifests: normal and build
dependencies decide it, and **dev-dependencies are added to it** so that a crate
whose tests use a sibling still packages (a dev-dependency that has a `version`
is resolved against the registry at publication). The graph, edges of every kind
included, has no cycle. Crates of one level do not depend on each other and can
go in any order.

| Level | Crates |
|---|---|
| 1 | `energyml-derive`, `energyml-build-derive`, `energyml-log`, `energyml-geostat` |
| 2 | `energyml-core` |
| 3 | `energyml-eml-2-0`, `energyml-eml-2-1`, `energyml-eml-2-2`, `energyml-eml-2-3` |
| 4 | `energyml-resqml-2-0-1`, `energyml-resqml-2-2`, `energyml-resqml-2-2-dev3`, `energyml-witsml-2-0`, `energyml-witsml-2-1`, `energyml-prodml-2-0`, `energyml-prodml-2-2`, `energyml-prodml-2-3` |
| 5 | `energyml-schemas` |
| 6 | `energyml-storage` |
| 7 | `energyml-domain` |
| 8 | `energyml-geometry`, `energyml-proj`, `energyml-sheet`, `energyml-rules` |
| 9 | `energyml-analysis`, `energyml-cache`, `energyml-export` |
| 10 | `energyml-section` |
| 11 | `energyml-build` |
| 12 | `energyml-providers` |
| 13 | `energyml-cli`, `energyml` |

Why `energyml-schemas` is so early: `energyml-storage` and the others use it in
their tests. Looking at normal dependencies alone, `energyml-schemas` would come
after `energyml-storage`, and `energyml-storage` could not be packaged.

Everything that depends on the generated crates must wait for **all** of them
(`energyml-schemas` lists every release). The ETP stack lives in the
[`retp`](https://github.com/geosiris-technologies/retp) repository and depends
on `energyml-core`, `energyml-storage` and `energyml-schemas`: publish those
three first, then follow `retp`'s own order.

`release-plz` computes this order itself once the crates are on the registry.
The order matters for the **first** publication of each crate, which is done
once, in order, by a maintainer with a short-lived API token; trusted publishing
is then configured per crate on crates.io (owner `geosiris-technologies`,
repository `energyml-rust`, workflow `release-plz.yml`, environment `crates-io`)
and every later release goes through the workflow. The `crates-io` environment
only accepts deployments from `main` and `dev`.

A pre-release goes out from `dev`: set the version by hand (for example
`0.2.0-alpha.1`) and push. The `release` job publishes every crate whose version
is not yet on crates.io and tags it; it opens no release pull request and starts
neither the CLI binaries nor the image. Cargo never picks a pre-release unless
it is asked for by name (`energyml = "0.2.0-alpha.1"`).

A tag pushed with the workflow token starts no workflow on its own, so the
`release` job of `release-plz.yml` starts `release-cli.yml` and `docker.yml`
itself (`workflow_dispatch` on the new `energyml-cli-v*` tag). The publish gates
(`RELEASE_PLZ_PUBLISH`, `IMAGE_PUBLISH`) apply as before.

Before the first publication, check what would be packed, without publishing:

```sh
cargo package --workspace --no-verify --allow-dirty   # what CI's `package` job does
cargo package --list -p energyml-domain               # the tables must be listed
```

`--no-verify` is needed in CI because a crate cannot be built from its packed
sources while the crates below it are not on the registry yet. Once they are,
drop it.

## The dev-dependency cycle between `energyml-rules` and `energyml-domain`

`energyml-rules` uses `energyml-domain` in its tests (it crosses the rules of
the registry with `energyml_domain::integrity::checks()`). Today
`energyml-domain` does **not** depend on `energyml-rules`, so there is no cycle
at all, and the order above puts `energyml-rules` after `energyml-domain`.

The risk is the day `energyml-domain` starts to depend on `energyml-rules`:
crates.io refuses a dependency cycle only between normal dependencies, but a
dev-dependency **with a `version`** is kept in the published manifest and must
be found on the registry when the crate is packaged, so neither crate could be
published first. The dev-dependency of `energyml-rules` therefore has **no
`version`** (`energyml-domain = { path = "../energyml-domain" }`): `cargo
publish` removes such a dev-dependency from the published manifest, and the two
crates are independent at publication whatever their order. Do not add a
`version` to it. The same remedy applies to any dev-dependency that would close
a loop.

## Python wheels

The metadata of the wheels is in
[`crates/energyml-python/pyproject.toml`](../crates/energyml-python/pyproject.toml)
(distribution `energyml-rust`; the version is read from the crate). Pushing the
tag `python-vX.Y.Z` builds the wheels and the sdist, and uploads them by PyPI
trusted publishing. The tag must equal the version of the crate; the `check-tag`
job fails otherwise.

## The documentation build

docs.rs builds each crate on Linux with the features of its
`[package.metadata.docs.rs]`, with `--cfg docsrs` and a nightly compiler. The
`docsrs` job of the CI does the same, so a crate that would not build there is
found before the release, not after.
