# Schemas and supported versions

## Supported versions

The library reads and writes these Energistics data schemas. Every version is a
separate generated crate under `generated/`, and `energyml-schemas` registers
them in one `Registry` (`energyml_schemas::all()`, or `energyml::schemas::all()`
through the `energyml` crate) - those whose Cargo feature is on.

| Standard | Versions |
|---|---|
| RESQML | 2.0.1, 2.2 (and the development drop `2.2_dev3`, see below) |
| EML (Energistics common) | 2.0, 2.1, 2.2, 2.3 |
| WITSML | 2.0, 2.1 |
| PRODML | 2.0, 2.2, 2.3 |

## Choosing the versions you compile

Each version is a Cargo feature, spelled as Energistics spells a qualified type:

| Standard | Versions | Features |
|---|---|---|
| EML | 2.0, 2.1, 2.2, 2.3 | `eml20`, `eml21`, `eml22`, `eml23` |
| RESQML | 2.0.1, 2.2, 2.2_dev3 | `resqml20` (the 2.0.1 release), `resqml22`, `resqml22dev3` |
| WITSML | 2.0, 2.1 | `witsml20`, `witsml21` |
| PRODML | 2.0, 2.2, 2.3 | `prodml20`, `prodml22`, `prodml23` |

The families `eml`, `resqml`, `witsml`, `prodml` and `all` switch several on. A
version brings the EML version it is built on (`resqml22` brings `eml23`,
`resqml20` brings `eml20`). The `energyml` crate turns **no** version on by
itself, so a dependency says what it reads:

```toml
energyml = { version = "0.1", features = ["resqml22"] }   # or ["all"]
```

`energyml-schemas` itself defaults to `all` (for the tools that want
everything); the `energyml` command and the Python wheel ship every version. A
version that is not compiled is a named refusal ("RESQML 2.2 is not compiled
in: enable the `resqml22` feature"), never a half read. The façade's `build`
feature (and `providers`, which implies it) turns on the four versions it
writes: RESQML 2.2, EML 2.3, WITSML 2.1 and PRODML 2.3.

Everything above the generated layer - units, coordinate systems, value arrays,
geometry, exports - is written **once for all versions**. A concept keeps its
version-independent name, so a reader for a concept serves every release that
keeps it; a release that respells the inside of a type is handled by a reader
that accepts both spellings.

**The development drop `resqml 2.2_dev3`** is registered so that files written
against it before the 2.2 publication still open. It is read, never offered: no
new object is created in it, because most of its types carry the very qualified
type of a released one, and the rest exist only in the drop and no released 2.2
reader knows them. `energyml relabel` can correct a package that declares the
release while holding drop content.

Adding an Energistics release costs a generated crate, one dependency, one
feature, one row of `KNOWN_VERSIONS` and one `register` line in
`energyml-schemas`; the storage, domain and geometry layers name no version. A test checks that the registry and the list of versions the
code is generated from agree.

## Where the generated code comes from

The Rust under `generated/` is produced **mechanically from the official
Energistics XSD files by an internal tool** that is not part of this repository.
Generated files are committed and are never edited by hand; a robot proposes
changes to them as pull requests. `generated/schemas.lock` records the SHA-256 of
each input schema so a reviewer can tell which input a file was generated from.
It contains no Energistics schema content.

The generated code is public because it is a derived work, produced under the
Energistics Product License Agreement; the XSD files themselves are obtained from
the official publisher (The Open Group) and are not redistributed here.

Each generated crate carries a `NOTICE`. TODO(verify): final wording and any
condition The Open Group or Energistics attaches to naming crates after their
standards.

## Notice

> This product includes software developed by Geosiris. The following
> Energistics(R) products were used in the creation of this work: RESQML(TM) Data
> Schema v2.0.1 and v2.2, WITSML(TM) Data Schema v2.0 and v2.1, PRODML(TM) Data
> Schema v2.0, v2.2 and v2.3, Energistics common v2.0-v2.3, Energistics Transfer
> Protocol (ETP) v1.2. Energistics(R), WITSML(TM), PRODML(TM) and RESQML(TM) are
> trademarks or registered trademarks of Energistics Consortium, Inc. All rights
> in the Energistics standards, or any portion thereof, which remain in this work
> remain with Energistics and are subject to the Energistics Product License
> Agreement (https://www.energistics.org/product-license-agreement).

TODO(verify): this text follows the licence's requirement for derived works;
confirm it against the NOTICE file of the repository, which is authoritative.

## Disclaimer

This project is **not affiliated with or endorsed by Energistics or The Open
Group**. RESQML, WITSML, PRODML and Energistics are trademarks of their owners.
"EnergyML" is used here as the common name of the family of standards.

## Related

The ETP (Energistics Transfer Protocol) implementation lives in a separate
repository, [`retp`](https://github.com/geosiris-technologies/retp).
[ADR 0001](adr/0001-generated-code-is-committed.md) explains why generated code
is committed.
