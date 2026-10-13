# energyml-derive: architecture

## Purpose and position

Procedural macros that turn the Rust structs emitted by the generator into implementations of the `energyml-core` traits: XML and JSON (de)serialisation, reflection, validation, identity accessors. The generator emits annotated Rust and nothing else; this crate gives it meaning. You normally depend on it through the default `derive` feature of `energyml-core`.

Related decisions: [ADR-0001](../../docs/adr/0001-generated-code-is-committed.md), [ADR-0008](../../docs/adr/0008-inheritance-and-xsd-list.md).

## Rules

**All behaviour lives here, not in the generated code** - changing how an `xsi:type` is written must be a change to one file in this crate, not a regeneration of twelve generated crates. The generator therefore emits constants and annotations; the derive emits trait impls.

**Three derives** - `EnergymlObject` (complex type; cardinality inferred from `Option<T>` / `Vec<T>`, `min`/`max` only when the schema says something the type cannot), `EnergymlEnum` (simple type enumeration; `#[energyml(open)]` with an `Other(String)` variant for the open `...Ext` types) and `EnergymlChoice` (abstract type or substitution group, selected by `xsi:type`; transparent to reflection).

**Generated impls stay small** - there are about a thousand derived impls, and default trait bodies are recompiled for each. The derive emits only `#[inline]` field accessors for identity (six of them) and leaves default bodies as one-line calls into `energyml-core`, where the shared code is compiled once.

**Field roles are detected from names** - case- and separator-insensitively (`UUID`, `Uuid`, `VersionString`, `ObjectVersion` need no special case anywhere), with `#[energyml(role = "...")]` as an explicit override. A `uid` counts as an identifier only on a top-level or reference type.

**Children are matched on namespace and local name** when a struct has two same-named children, because WITSML types carry their own `Citation` next to the inherited one; matching on the local name alone reads one into the other.

## Deliberately not here

- No schema knowledge, no knowledge of any release, no runtime logic beyond what an impl must contain; the runtime is `energyml-core`.
- No dependency other than `syn` and `quote`. The manifest spells out its package metadata instead of inheriting it, because the WebAssembly packaging tool does not resolve workspace inheritance for path dependencies.

## How it is tested

The derives are exercised by `energyml-core`'s `generated_shape` test (a hand-written model in the exact shape the generator emits, which is the specification of generator output) and by the workspace tests over the real generated crates.
