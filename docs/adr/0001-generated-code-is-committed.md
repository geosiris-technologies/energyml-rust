# 0001 - Generated code is committed, produced by an internal tool

Status: accepted

## Context

The Energistics standards are published as XSD files. A Rust type per element
(about twelve crates, one per package and version) cannot be written by hand and
kept in step across releases. The schemas themselves are distributed by their
publisher under a licence that allows derived works but not redistribution of the
schemas unmodified; the generator and its inputs are therefore not part of this
repository.

## Decision

- Rust types are **generated** from the XSD files by an internal tool and the
  output is **committed** under `generated/`, one crate per package and version.
- Nobody edits generated files by hand. A robot proposes regenerations as pull
  requests; CI of the producing side checks that committed output equals a fresh
  generation.
- `generated/schemas.lock` records the SHA-256 of every input schema. It carries
  no schema content.
- Each crate carries a notice crediting the Energistics products used.

## Consequences

- Building this repository needs no XSD, no generator and no network.
- A reviewer sees exactly how a schema release changes the Rust types.
- Contributors cannot fix a generated type here; the fix goes to the generator
  and arrives as a regenerated crate.
- The generated code is a derived work and carries the publisher's notice.
  See [Schemas](../schemas.md).
