# 0002 - The storage trait is drawn after ETP

Status: accepted

## Context

Data lives in EPC files today and will live behind the Energistics Transfer
Protocol (ETP) in remote dataspaces. If the storage abstraction were drawn around
a zip file, adding a network transport would force a rewrite of everything above
it.

## Decision

`EnergymlStorage` is designed from the ETP v1.2 requests: addressing by URI,
plural operations, partial responses. An EPC fits inside that shape - a file is a
dataspace.

- The trait is async; `EpcStore` also has a full synchronous API, because a
  WebSocket cannot block in WebAssembly and a program reading local files must
  not pay for async. A test compares both faces so they cannot drift.
- The trait only does I/O. Graph building, `.rels` regeneration and array location
  are free functions over objects already in memory, so no logic has a sync and an
  async copy.
- Three levels of materialisation: identity from the index, citation from a
  parser that stops, then the whole object. A listing of a multi-gigabyte package
  inflates no part in full.
- Tolerant and vocal: departures from the specification are accepted and recorded
  as diagnostics, never swallowed.
- Local-only capabilities (raw attachments and `.rels`) are a separate trait,
  because they make no sense in a dataspace.

## Consequences

- An application written against the trait changes transport by changing the line
  that builds the store. The ETP client ([`retp`](https://github.com/geosiris-technologies/retp))
  implements the same trait.
- Some conveniences of a local zip are deliberately not on the trait.
