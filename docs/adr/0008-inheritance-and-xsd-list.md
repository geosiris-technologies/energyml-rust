# 0008 - Inheritance and `xsd:list`

Status: accepted

## Context

The schemas use type extension and restriction heavily, and `xsd:list` types that
store a whole array as whitespace-separated text in one element. Rust has no
inheritance, and writing a list as one element per value produces a document no
other tool reads.

## Decision

- **Inheritance is flattened at generation time.** Chains of extensions are copied
  base-first into each concrete struct, across crates, so a RESQML object carries
  the fields of its Energistics-common base while the field types still point at
  that crate. A `base` relation is emitted for each abstract ancestor in the same
  crate, so a question like "is this an `AbstractRepresentation`?" is answered by
  reflection. The chain stops at a crate boundary by design.
- Restriction overrides inherited fields rather than duplicating them.
- **`xsd:list` is one element**: the whole array lives in the text of a single
  element. The generator marks such fields as lists, which stops the writer from
  emitting one element per number.
- What an object inherits that is not structural - a colour, a graphic
  information - follows an explicit, ordered lookup (see ADR 0009).

## Consequences

- Generated types are plain structs; reflection supplies the hierarchy.
- A test guards that every abstract ancestor is reachable, because a missing one
  would answer "false" silently for every surface.
