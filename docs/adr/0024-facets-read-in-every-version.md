# 0024 - Facets are read in every version, and say whether the schema lists them

Status: accepted

## Context

A property or a table column carries facets: qualifiers such as the direction
(`I`, `J`, `K`), the side, the statistics, the net-to-gross. The releases write
them with the same two element names, swapped:

- RESQML 2.0.1 writes the kind in `Facet` and the value in `Value`, a free text
  (RESQML TRG 2.0.1 section 6.36, p. 77).
- EML 2.3, and so RESQML 2.2, write the kind in `Kind` and the value in `Facet`,
  an open enumeration, `FacetExt` (EML TRG 2.3 section 11.11, p. 294).

A reader that knows one of the two reads nothing, or the wrong thing, in the
other, and there were two copies of it, one for properties and one for table
columns. The open enumeration also lets a producer write a value the standard
does not list: the `Other` hole of an `EnumExtensionPattern`, whose pattern is
`.*:.*` (EML TRG 2.3 section 6.28, p. 104 and section 11.2, p. 284). What such a
value means is the producer's, and a check that compares it to the standard's
literals would judge something nobody defined.

## Decision

- **One function reads the facets of any object** that has a `Facet` field,
  property or column, whatever the release. It tells the two forms apart by the
  field that exists (`Kind` or `Value`), and returns the facets in document order,
  without inventing, sorting or dropping any.
- **A facet is a kind and a classified value.** `Facet { kind, value }`, with
  `value` a `FacetValue`: `Standard(text)` when the schema lists it, `Extension(text)`
  when it does not. `FacetValue::as_str` is the one way to read the text, and
  `is_extension` the one way to ask the question.
- **Standard or extension, per release.** In EML 2.3 and later the schema says:
  a value of the generated enumeration is `Standard`, the `Other` hole is an
  `Extension`. In RESQML 2.0.1 `Value` is free text, so the rule of the pattern
  `.*:.*` is applied: a value with a namespace prefix (`nwdf:vertical`) is an
  `Extension`, any other is `Standard`. This is an interpretation, written here
  and not in the standard.
- **A table URI written as a facet value is an extension.** The convention that
  links a column to another object through a `qualifier` facet carries a URI, which
  has a colon: it is read as an `Extension`, which is what it is.
- **Reading is not judging.** The business rule "do not use several facets of the
  same kind for the same property" (EML TRG 2.3 section 11.11, p. 294) is not
  checked by the reader; the rules that need it decide.

## Consequences

- A facet on a 2.0.1 property and on a 2.2 property read the same, and the
  directions of a grid property, its side or its qualifier are available to every
  view without knowing the release.
- A rule that wants the standard's own value (a direction `K`, a ratio) can say
  "not checkable" on an extension instead of guessing; this is
  [ADR 0005](0005-refuse-rather-than-draw.md) applied to a vocabulary.
- `Facet.value` changed from a text to a type: a caller that read the text uses
  `facet.value.as_str()`. No caller in this repository read it before.
