# 0022 - The kind of an edge comes from the specification, labelled; the schema facts are generated

Status: accepted

## Context

ETP 1.2 calls each relationship between two data objects (a data object
reference, or by-value containment) **primary** or **secondary** (section
8.1.1.1.2, p. 157), and a discovery can follow only one kind (`navigableEdges`)
or widen its answer to the secondary neighbours. Which kind a relationship has
is left to "the ML-specific ETP implementation guide", written by the ML of the
object that holds the reference (or contains the other).

Those guides are not published for ETP 1.2. The schemas do not label the kind
either; the specification says future versions of the data models will. A store
that derives its edges from the objects (an EPC) therefore stated no kind, and a
discovery by kind over it was refused for the whole dataspace, which also
refused every such request a server built on the storage traits relayed.

The specification does state some rules:

- **RESQML**: "In RESQML, all relationships are primary" (section 8.2.2, p. 167-168).
- **WITSML**, three examples: a `ChannelSet` contains `Channel`s and a `Log`
  contains `ChannelSet`s (primary, by value); the reference from a `Channel` to
  its `Wellbore` is secondary (p. 157).
- The "common characteristics" (p. 157): a primary relationship has one end
  that is mandatory, or is by-value containment; a secondary one has both ends
  optional.

## Decision

- **The facts are generated.** For each top-level type of each schema version,
  the generator writes where the type holds data objects by value and where it
  writes references, with the cardinality of every step
  (`energyml_core::relations`, one `relations.rs` per generated crate). The
  facts are checked against the schemas by an independent reader and against
  the reflection of the generated types (ADR-0005: never one source alone).
- **The policy is written once, by hand, and every answer says what it rests on**
  (`energyml_storage::kinds`, `KindBasis`): `EtpSpec` for RESQML and the three
  WITSML examples, with the section and page; `DerivedXsd` for another WITSML
  reference (primary when the whole path to it is mandatory, secondary
  otherwise) and `DerivedByValue` for other WITSML containment, both labelled as
  derived, never presented as the ML's rule; `Stated` for what a server sends;
  `Unknown`, with the reason, for PRODML, EML on its own, and any other ML.
- **An edge with no kind is refused where it is met** (`Error::UnstatedKind`),
  not dataspace-wide: a request by kind answers as long as its walk meets only
  edges that have one.
- **The kinds are given in one place**, the provided `EnergymlStorage::edges`,
  through the hook `kind_rules`; `Capabilities::edge_kind_coverage` says of which
  MLs the kinds are stated.

## Consequences

- An EPC holding RESQML answers every discovery by kind; a server built on the
  storage traits relays it, and a client reads the same edges back with their
  kinds (the triangulation of such a server agrees on every context of its
  matrix).
- A derived kind can be wrong for an ML whose guide would say otherwise. It is
  labelled as derived wherever it is shown, and replaced when a guide, or a
  version of the schemas that labels its relationships, says otherwise.
- A request by kind that meets a PRODML edge is refused by name until a rule
  for PRODML exists.
