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

## Addendum: what the door covers

- `ObjectId::from_etp_uri` (and `ObjectId::parse`) keep no reading of a URI of
  their own: they call `EtpUri::parse` and build the identity from its answer. So
  what the door refuses they refuse - a uuid that is not a uuid, a type with a
  character a type cannot carry - and the uuid comes back in lowercase. A URI that
  names a dataspace and no object, or that goes on below the object, is refused by
  name, because an identity cannot hold it.
- The qualified type and the content type each have one grammar
  (`identity::parse_qualified_type`, `identity::split_content_type`); the owned
  `QualifiedType` and `ContentType` of `identifiers` are wrappers around them.
- Two addresses are compared by `identity::same_address`: same uuid, versions that
  do not contradict. An address that names no object (an empty text, a dataspace)
  is the same as nothing, not even as itself.
- A reference whose `ContentType` says `version=2.0.1` and the object it points
  at have the same uri: `Registry::canonical_key` and
  `Registry::canonical_qualified_type` normalize a version to the schema version
  the registry declares (`2.0`), and refuse a spelling the registry does not know,
  by name. `EpcStore` resolves through them.
- A quote inside a dataspace name is written twice in a URI (ETP 1.2 section
  25.3.10), as for the object version: `EtpUri::dataspace_uri`, `object` and
  `Display` double it, `EtpUri::parse` undoes it. A dataspace named `it's` is a
  usable name.
