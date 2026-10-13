# Testing

## What the public test suite stands on

No public test depends on files nobody versions or reviews. The suite is built
from four kinds of data:

| Source | Covers | How |
|---|---|---|
| **A. The reference model Lutetia** | the library's functions on RESQML 2.2, EML 2.3, WITSML 2.1 and PRODML 2.x: geometry, properties, colours, tables, section view, export, Python, CLI | the model is a versioned repository, [`energyml-lutetia`](https://github.com/geosiris-technologies/energyml-lutetia), read at a **pinned commit** (`tests/lutetia.lock`); in CI `scripts/fetch-lutetia.py` fetches it into `target/lutetia`, locally it is a neighbouring folder or `ENERGYML_LUTETIA_DIR`; with neither, its tests skip (a CI job runs the whole suite that way) |
| **B. Pinned reference packages from other tools** | **interoperability** and RESQML 2.0.1: packages written by other software keep the cross-checks honest | fetched **in CI only** from their original repositories, each listed in `tests/fixtures.lock` with repository, commit, path and SHA-256. A file whose hash differs fails CI; it is never accepted silently. No test goes on the network |
| **C. Data fabricated inside the tests** | edge cases and refusals: absent values, mistyped values, byte order, ... | built by the test itself |
| **D. (optional) Packages written during CI by other libraries** | widening B without storing files | only if A, B and C leave a measured gap |

The packages in B are small example packages: those written by the example
programs of FESAPI (RESQML 2.0.1 and 2.2, in the Geosiris `energyml-python`
repository) and those of resqpy (RESQML 2.0.1, MIT licence). TODO(verify): final
list and licences in `tests/fixtures.lock`.

```sh
scripts/fetch-fixtures      # the same as CI does, on demand, into target/fixtures/   (TODO: script)
cargo test --workspace --test tests          # the end-to-end binary
cargo test --workspace <name-filter>         # one test by name, across the workspace
```

A test whose data is missing **fails in CI** instead of skipping. Locally it may
skip, and the summary says how many suites did.

## Rules the tests follow

- **The shape of an assertion comes from the specification, the values from the
  file.** A real producer may write non-conforming data, and pinning a measured
  value turns that defect into a rule the suite then defends. Where a test states
  a rule, a comment names the document, page and paragraph; the document itself
  is never opened at run time.
- **A test must be able to fail.** When adding one, ask what you would have to
  break for it to turn red; if the answer is "nothing", break something on purpose
  to find out. Empty sets pass any assertion.
- **Triangulate.** Never ask a component to check itself: each cross-check
  confronts two independent sources of the same fact, at least one not written
  here - the bytes of the HDF5 file, the part names of the zip, the counts the XML
  declares. Cross-checks live in `crossaudit.rs`.
- **One crate, one test binary.** Test files are modules of a single
  `tests/main.rs` per crate; a new test file is one `mod` line, or it does not run.
  A file that touches process-wide state keeps its own binary.
- **Do not run `cargo test -p <crate>`**: feature resolution for a single crate
  differs from the workspace and rebuilds everything. Filter inside the workspace.

## What is not covered here

The suite above is honest about its limits: Lutetia is written by this project's
own operations, so on its own it checks functions, not interoperability - which is
what B is for. A **private campaign on real-world packages** (round trips,
cross-audits, the command line) also runs on the maintainers' side; its files are
not redistributable, and its results are not a public guarantee.
