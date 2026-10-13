# 0009 - Properties, tables and colours: one colour rule

Status: accepted

## Context

A property's values are numbers in an array; what they mean depends on a kind, a
unit, a lookup table for discrete values and, for display, a colour. The
specification defines none of the priority between several colours that may apply
to one object (its own, its property kind's, its interpretation's, a collection
that references it, a parent's).

## Decision

- Property kinds, lookup tables (discrete and categorical properties) and colour
  maps are resolved once, in the domain layer, for every schema version.
- A property's value count is checked against the geometry it hangs on; a
  property lined up against the wrong elements is reported, not drawn.
- **One colour rule for every face.** The colour of an object is decided by one
  cascade, in one place (`energyml_cache::colours`), which returns the level that
  answered and the object it came from. Order of search: directly on the object;
  then collections that reference it (recursively); then its direct parents; then
  parents through collections; then up the parents; finally the model and its
  collections. Among several paths, the shortest wins; on a tie, the first.
- The decision is visible: a decision path can be shown rather than only a result.
- No default colour ramp is invented when the package states none.

## Consequences

- The command line, Python and any viewer give the same colour for the same
  object.
- The rule is a house rule, listed in the registry, and can be disagreed with
  explicitly.
