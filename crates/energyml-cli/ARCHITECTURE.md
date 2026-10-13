# energyml-cli - architecture

Related decisions: [ADR 0012](../../docs/adr/0012-no-face-carries-logic.md) (no face carries logic), [ADR 0006](../../docs/adr/0006-registries-rather-than-match.md), [ADR 0003](../../docs/adr/0003-identity-full-uri-one-door.md), [ADR 0010](../../docs/adr/0010-package-index-and-caches.md), [ADR 0016](../../docs/adr/0016-one-crate-one-test-binary.md).

## Purpose and position

The `energyml` command (also installed as `enyml`, same code): list, check, show, export, repair, rename, extract, select, create, fetch and the other subcommands. It is the top of the stack: a library crate (`energyml_cli::run`) plus two thin binaries, `energyml` and `enyml`, that only call it.

## Rules

- **A face parses arguments and prints; it decides nothing.** Every check, filter, operation and report lives in a library crate, so the command line, the Python bridge and any embedding answer the same. A face that knows something the library does not is a split bug. When logic is found here, it is moved down and the command keeps the display; its output must stay byte-identical.
- **The CLI is the only place a schema version is wired.** The storage layer receives a registry and never builds one. The registry here is one line over `energyml-schemas`, the single list of releases, and a test fails when that list is stale.
- **External subcommands.** `energyml foo …` runs the program `energyml-foo` found on `PATH`, the way `git` and `cargo` do (`src/external.rs`). Other tools of the family are separate programs, installed and published on their own; nothing here names their crates, so a build never depends on them. Two words are aliases for programs that do not follow the pattern: `etp` runs `retp`, and `translate` runs the translation program. A missing program produces an installation hint. The lookup takes the `PATH` as an argument, so it is tested without spawning anything.
- **Caching is a constant per command.** By default a command keeps nothing between two questions (`Cache::off`). Only the commands on the fixed list in `src/layers.rs` (those that colour: `colors`, `export`, `geometry`) turn on the package index and colour walks. That is also what gives the command line the colour rule of every other face. A command not on the list keeps nothing.
- **Network I/O stays in the face.** `create --from <url>` and `fetch` download through a blocking HTTP client here, never in `energyml-build` or `energyml-providers`, which stay parsers and vocabularies and compile where there is no network.
- **One door per access.** Objects are found through the store's lookup, identities parsed through the URI parser; a `#<uuid>` on the command line is an input adaptation that becomes a full identity immediately.
- **Heavy dependencies are optional.** `hdf5` (needs the HDF5 C library at build time; without it a request for array values refuses and names the feature, never returns an empty array) and `png` are off by default.
- **The reference is generated.** `docs/cli-reference.md` is produced from the command definitions by a script; CI checks it is not stale.
- **Refusals are named and the exit code means something.** `check` exits 1 on errors; an export that cannot write an object says so and moves on to the next (a directory shows what is missing), whereas library calls that return one document refuse.
- **Galaxy tools are written from operation declarations** (`galaxy-tool`, hidden). The default tool image is `ghcr.io/geosiris-technologies/energyml`; the executor script is a contract shared with workflows that call it.

## Tests

The library behind each command is tested where it lives; here the tests cover argument handling, the external-command lookup, the fixed cache list and the generated reference. Run the workspace suite, not `cargo test -p energyml-cli` alone, so feature resolution matches the whole workspace.
