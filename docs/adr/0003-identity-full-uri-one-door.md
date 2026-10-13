# 0003 - Identity: full URI internally, one door per access

Status: accepted

## Context

An object can be named by a bare uuid, a short URI or a full URI that carries its
version. A uuid alone is ambiguous when two versions of an object coexist, and a
library with several parsers for the same identifier ends up with several answers
to "which object is this?".

## Decision

- **Inside the library, only full URIs** are used as keys and in comparisons, and
  the URI carries the version when the object declares one. A URI is built from an
  object or a reference in exactly one way (`ObjectId::of_reflect`, then
  `etp_uri`).
- A bare uuid or a short URI is accepted **only at the edge** (adapters such as
  `lookup_uuid` and `resolve`) and turned into a full identity at once.
- **One door per access.** A URI is parsed by `EtpUri::parse`; an object is found
  by `EpcStore::lookup`; a colour is decided by `energyml_cache::colours`. A second
  implementation, even private, is a defect: two faces would end up answering two
  different things.
- A view that shows an object's type and title also shows its version when it is
  neither empty nor absent.

## Consequences

- Several versions of one object can live in a package without being confused.
- Adding a feature never adds a second identity parser; known exceptions are
  tracked and removed.
- Convenience for users remains at the entry points.
