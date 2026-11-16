# 0042 - What a property says of itself

Status: accepted

## Context

A view of a property shows its numbers (bounds, scale, variation along each
axis) and, beside them, what the **file** declares about it: its shape, its
facets, its unit, its mark of synthetic data. The standards describe the
dimensions of the values and the facets, but they leave several readings open,
and a library that does not choose lets each view choose for itself. Six of
those readings had to be settled for `property::chunks` and
`property::observe`.

## Decision

- **The dimensions reduced by `AlongReduced` are reduced together.** The pick
  keeps every component along one axis, fixes the other dimensions, and
  reduces the listed ones (`mean`, `min`, `max` of the finite values, by
  `Reduction::apply`, the one arithmetic of a reduction). A mean over two
  reduced dimensions is the mean of the finite values of the whole sub-block,
  not a mean of means: with a missing value the two differ, and the first is
  the mean of the values that exist.
- **A property that declares several values per element and holds one is
  described as scalars, under `ComponentPick::All` only.** The rule is the one
  the scene already applies (declaration wrong, values usable). Under any other
  pick the property is refused (`ChunksRefusal::OnePerElement`): a component
  cannot be chosen among values that do not exist. The observation
  `DeclaredCountWrong` says it.
- **The list of facet values of EML 2.3 does not bind a RESQML 2.0.1 value.**
  In 2.0.1 the value of a facet is free text (RESQML TRG 2.0.1 §6.36 p. 77).
  In 2.2 a standard value found under a facet kind other than the one the
  schema documents for it is said (`FacetValueOfOtherKind`); in 2.0.1 nothing is
  said. Several facets of one kind are said (`FacetKindRepeated`, EML TRG 2.3
  §11.11 p. 294), and not refused.
- **The mark `synthetic` is read as `true`, `false` or something else.** An
  `ExtensionNameValue` named `synthetic` (ADR 0019) is a mark when its value is
  `true`, no mark when it is `false`, and `Unclear` otherwise, with the value as
  written. It is read on the property and on its supporting representation. It
  is never deduced from a title, and nothing is said of an object that carries
  none: the absence of a mark is not a proof that the data is measured.
- **The observations of a property live beside those of its values.**
  `stats::describe::Observation` says what the values are (counts, scale, modes,
  axes) and is computed while they are read. `property::observe::PropertyObservation`
  says what the file declares and is computed without reading a value. No fact
  is written twice. The unit and the kind are carried by `UnitCheck` (checked,
  outside the quantity class, or not checkable and why) and are never reported
  as checked when the class is one this build does not convert.
- **The ratio of two sister properties is a figure, not a judgement.**
  `PairWalk` counts the pairs, the density, the ratio `y / x`, the share that
  satisfies a rule the caller gives, and the correlation. That a share is a
  warning, and which sister is the vertical one, is the rule
  `house/property/directional-ratio`, not this code.

## Consequences

- A view that shows a property has two lists to show, the observations of the
  values and those of the file, and neither repeats the other.
- A defect of the file (a repeated facet, a wrong declared count) is reported,
  and the property stays readable: the library helps repair such files rather
  than refuse them.
- A new kind of observation is a new variant (`PropertyObservation` is
  `#[non_exhaustive]`), not a change to the observations of the values.
- The reading of a 2.0.1 facet value stays open to a producer's own words, at
  the price that a misplaced standard value there is not caught.
- `TODO(verify)`: whether the packages in the field carry the `synthetic` mark;
  until they do, most properties are silent on it.
