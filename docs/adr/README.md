# Architecture decision records

Short records of decisions that shape this library and the reasons behind them.
Each one has three parts: **Context** (what forced a choice), **Decision** (what
was chosen) and **Consequences** (what it costs and what it buys). They are
written to be read in a minute; the code and the other pages are the detail.

A record is not edited to change its meaning. A decision that is reversed gets a
new record that supersedes the old one, which keeps its text and a status line.

| # | Decision |
|---|---|
| [0001](0001-generated-code-is-committed.md) | Generated code is committed, produced by an internal tool |
| [0002](0002-storage-trait-follows-etp.md) | The storage trait is drawn after ETP |
| [0003](0003-identity-full-uri-one-door.md) | Identity: full URI internally, one door per access |
| [0004](0004-reading-policy.md) | Reading policy: `ReadMode`, version resolution, tolerance |
| [0005](0005-refuse-rather-than-draw.md) | Refuse rather than draw; `FeatureNotEnabled` |
| [0006](0006-registries-rather-than-match.md) | Registries rather than `match` |
| [0007](0007-core-has-no-native-dependency.md) | `core` has no native dependency (WebAssembly) |
| [0008](0008-inheritance-and-xsd-list.md) | Inheritance and `xsd:list` |
| [0009](0009-properties-tables-colours.md) | Properties, tables and colours: one colour rule |
| [0010](0010-package-index-and-caches.md) | The package index and caches |
| [0011](0011-tests-follow-the-specification.md) | Tests follow the specification, not a corpus; triangulation; a test must be able to fail |
| [0012](0012-no-face-carries-logic.md) | No face (Python, CLI) carries logic |
| [0013](0013-witsml-channels-in-hdf5.md) | WITSML channels in HDF5 |
| [0014](0014-horizons-units-attachments-constraints.md) | Horizons and units, attachments, business constraints |
| [0015](0015-section-view.md) | The section view: three stages, `model.rs` as contract, one drawer |
| [0016](0016-one-crate-one-test-binary.md) | One crate, one test binary |
| [0017](0017-operation-is-a-pure-function.md) | An operation is a pure function; only `apply` writes |
| [0018](0018-synthetic-reference-model.md) | A synthetic reference model rather than an external corpus |
| [0019](0019-synthetic-data-is-flagged-and-sourced.md) | Synthetic data is flagged and sourced |
| [0020](0020-a-grid-is-never-reprojected.md) | A grid is never reprojected |
| [0021](0021-own-tool-trait-rmcp-as-adapter.md) | Our own `Tool` trait; the MCP SDK is an adapter behind a feature |
| [0022](0022-edge-kinds-from-the-specification.md) | The kind of an edge comes from the specification, labelled; the schema facts are generated |
| [0023](0023-objects-held-by-value.md) | The data objects held by value are listed apart, and write their own references |
| [0024](0024-every-edge-has-a-kind.md) | Every edge has a kind: EML and PRODML by the WITSML rule, primary by default otherwise |

Decisions about the ETP protocol implementation are recorded in the
[`retp`](https://github.com/geosiris-technologies/retp) repository.
