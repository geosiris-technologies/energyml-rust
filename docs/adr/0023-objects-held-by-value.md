# 0023 - The data objects held by value are listed apart, and write their own references

Status: accepted

## Context

Some data objects hold others by value: a WITSML 2.1 `Log` holds `ChannelSet`s,
which hold `Channel`s; an EML 2.3 `LocalEngineering2dCrs` holds its
`OriginProjectedCrs`. Each contained object is a data object of its own, with a
uuid, and ETP 1.2 lists it as one (section 9.1.3, p. 178: it "MAY be included in
one or more container data objects"). The relationship between the two is a
by-value containment whose source is the contained object (section 8.1.1.1.1,
p. 156), primary (section 8.1.1.1.2, p. 157).

An EPC holds only the container as a part. Reading it as before, the contained
objects did not exist, and every reference written inside one was given to the
part: a `Channel`'s reference to its `Wellbore` read as the `Log`'s, which states
an edge nobody wrote and judges its kind on the wrong type (`Channel` to
`Wellbore` is the specification's secondary example).

## Decision

- **`resources()` stays the parts.** The objects held by value come from
  `EnergymlStorage::contents()` (provided, empty), with one by-value edge from
  each to its direct container. `EpcStore` reads the container parts once, on
  demand, never when listing. `Capabilities::contained_objects` says whether a
  store lists them; discovery adds them when it does.
- **A contained object is a resource**: `ObjectSummary::contained_in` names its
  containers, `get_objects` and `headers` answer for it from the part
  that holds it. The same object in two containers is one resource with two
  edges; a contained object that is also a part is the part.
- **A reference has for source the object that writes it.**
  `EnergymlStorage::references` gives each DOR written inside a contained
  object to that object, its path relative to it
  (`graph::attribute_to_writers`), and `graph` draws the contained objects as
  present nodes with their containment edges. `graph::build` keeps giving every
  reference to the part: what a part depends on is what anything inside it
  names.
- **Writing refuses by name what it cannot do faithfully**: deleting a container
  whose contents would not survive, writing a contained object on its own.
  Pruning a container's contents is a separate capability
  (`Capabilities::prune_contained`), false for an EPC.

## Consequences

- A WITSML `Log` read from an EPC and the same `Log` read from an ETP server
  should answer the same edges, with the same sources and kinds: the contract
  `contract_witsml` is written against the trait, to be run against either.
- A package lists more objects than it has parts, and a graph drawn from it
  shows them.
