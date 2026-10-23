# 0024 - Every edge has a kind: EML and PRODML by the WITSML rule, primary by default otherwise

Status: accepted. Supersedes the `Unknown` row of [ADR-0022](0022-edge-kinds-from-the-specification.md).

## Context

ETP 1.2 requires every `Edge` to be `Primary` or `Secondary` (section 23.34.13).
Under ADR-0022 an edge whose source is EML or PRODML (or an ML with no rule)
had no kind, and a request by kind that met one was refused by name. Every real
package holds EML 2.3 objects (CRS, `PropertyKind`, `GraphicalInformationSet`,
`ColumnBasedTable`, `Activity`), so such a request over a whole package was
refused, and a server relaying the store could not answer it.

ETP leaves the rule to an ML implementation guide that is not published for EML
and PRODML (section 8.1.1.1.2, note 1). The same section characterises the two
kinds, p. 157: primary, "One end of the relationship is mandatory; that is, one
object cannot exist (as a data object in the system) without the other." and
"Relationships where one data object 'contains' one or more other data objects
(i.e., a by-value relationship)"; secondary, "Both ends of the relationships are
optional." And "the type of relationship is determined by the ML of the data
object that has the DOR".

## Decision

Taken by the maintainer on 23/10/2026.

- **EML and PRODML follow the rule WITSML already follows**: a DOR whose slot
  the XSD makes mandatory is primary, an optional one secondary
  (`KindBasis::DerivedXsd`); by-value containment is primary
  (`KindBasis::DerivedByValue`). The ML of the holder decides.
- **Where no rule applies and the schema tables hold no fact** (any ML, WITSML
  included, an unknown ML, a source that is not an object URI): **primary by
  default**, labelled `KindBasis::Default { reason }`, never confused with a
  rule of the specification or an XSD fact.
- Why primary rather than secondary: discovery follows primary edges by default
  (`navigableEdges`), so a primary edge called secondary would silently hide
  objects from a discovery, while the opposite error shows one object too many,
  which is visible; and RESQML's convention is all primary.
- Every edge keeps its basis, so a reader can always tell a default from a rule.
  An EPC reports `Capabilities::edge_kind_coverage` as `All`.

## Consequences

- A discovery by kind over an EPC answers whatever MLs it holds.
- The generator does not compute the reference slots of EML and PRODML yet
  (their `relations.rs` say "not computed"), so today their DORs are primary by
  default, not derived; they become derived, each with its XSD fact, when the
  generator computes them, with no change to the policy.
- A default can be wrong. It is labelled wherever it is shown (`kind_basis` in
  the MCP answers), and replaced when a guide, or a version of the schemas that
  labels its relationships, says otherwise.
- `KindBasis::Unknown` and `Error::UnstatedKind` remain for a store whose
  edges nobody judged and for a caller's own `KindRules`; the standard rules no
  longer produce them.
