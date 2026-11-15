# 0040 - The members of a wellbore are walked in the domain, from the inventory and the references alone

Status: accepted

## Context

Several views start from the same question: what does this well have? Its
trajectories, the frames that sample along it, its marker frames, the
properties that hang on them. A grid of wells asks it to show what each well
holds; the depth coverage asks it to know which representations to measure.
The question needs no array and no geometry: only the type, the uri and the
title of each object, and the references between them.

The walk existed once, in a desktop application, as a port of a web studio's
`wellMatrix`. A library that two faces and a coverage share cannot keep it in
one of the faces. The first design put it in `energyml-cache`, which holds the
package index. But holding the index is not reading it, and the depth coverage
(`md_extent`) lives in `energyml-domain`, which does not depend on the cache.

## Decision

- **The walk lives in `energyml_domain::well_members`**, beside the package index
  it reads, so that the cache, the faces and the coverage all call one
  function.
- **It is a pure function of facts**: `well_members(objects, outgoing)`, where
  `objects` are `ObjectFact`s (uri, qualified type, title) and `outgoing(uri)`
  gives the uris an object references. It is asked once per relevant object.
  `well_members_of_index` is the same walk over a `PackageIndex`, whose edges
  are its `references_of`. A dataspace of a server gives its own edges the same
  way.
- **A well is seeded from both ends**: a feature is a row even with nothing on
  it, and a representation that reaches no well makes its own row. A
  representation reaches its well by its interpretation and the interpretation's
  feature; a WITSML trajectory by its wellbore and the feature that names that
  wellbore. A reference to an object the list does not hold is ignored.
- **The kinds are a table** (`MEMBER_KINDS`), not a `match`: a type added later is
  a line. A type is read by its local name, without the `obj_` of release 2.0.1,
  so that the same concept is the same in every release.
- **A property is categorical when it references a table of categories**
  (`CATEGORY_TABLE_MARKERS`). The marks are carried over with the doubt of the
  code they come from: that a continuous or a discrete property never
  references a `ColumnBasedTable` is not verified.
- **The walk does not judge.** Whether a member can be drawn, the wording of a
  state, and the telling apart of two wells of one title are the business of the
  face. The order of the result is by lower-case title, then key.

## Consequences

- The grid of wells of the desktop application, the studio and the depth
  coverage walk the same members; a type added to the table is added for all.
- The domain gains a module and no dependency.
- Two wells of one title come out as two entries with the same title; a face
  that shows them must tell them apart.
