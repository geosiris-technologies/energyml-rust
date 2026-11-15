# 0036 - Two directional properties of one quantity keep the order a layered medium expects

Status: accepted

## Context

A property can be told apart from its sibling by a `direction` facet alone: a
resistivity logged along the layers and the same resistivity logged across them
(`Rh` and `Rv`), or the `kx`, `ky` and `kz` of a permeability. The facet kind
and its values are in the specification: `I` and `J` are the lateral axes of a
grid, `K` is its "third local grid (vertical) direction", `X`, `Y` and `Z` the
same on coordinates, each with a `+` and a `-` variant (common TRG 2.3 section
11.1 p. 281-282, `FacetKind` section 11.3 p. 285).

The specification says **nothing** about the order of the values of two
directions. What a reader expects is physics: in a layered medium, the current
crossing the layers meets more resistance than the current running along them, so
the vertical resistivity is not below the lateral one. On a real package of
about two million pairs, 38 % of the pairs had a vertical value under 0.95 times
the lateral one: a package that breaks the expectation is not rare, and it is not
necessarily wrong (noise, an inversion that models another medium).

Two more facts shape the rule:

- a direction can be written as an **extension value** (`nwdf:vertical`), which
  the standard leaves to the producer (common TRG 2.3 section 11.2 p. 284): which
  axis it names is not in the specification;
- the expectation is known for one quantity only. For a permeability it is weaker
  and unconfirmed.

## Decision

- **A warning, never an error** (`house/property/directional-ratio`,
  `Gravity::Warning`). The expectation is a reading of the house, not of the
  specification, so the finding says it and the package is kept.
- **Two thresholds**, written as constants of `integrity`: a pair breaks the order
  when the vertical value is under 0.95 times the lateral one (`RATIO_TOLERANCE`),
  and the rule warns when more than 5 % of the pairs do (`RATIO_SHARE`, compared
  with `>`: exactly 5 % is silent). A value equal to the array's own `NullValue`,
  a value that is not finite and a lateral value of zero are not pairs.
- **The siblings are found by the one rule of kinship**
  (`property::kinship::kinship`): same support, element, kind, unit and shape,
  `direction` facets that differ. A vertical is paired with each lateral of the
  **same family of axes** (the grid's `I`, `J`, `K`, or the coordinates' `X`, `Y`,
  `Z`): a grid's `K` against a coordinate `X` is not a comparison. The pairs are
  counted by `stats::tally`, the one counter.
- **A table of kinds, not a `match`**: `DIRECTIONAL_EXPECTATIONS` holds the
  resistivity (the dictionary uuid 7c0d1a5d-483e-4d77-81f3-9883527838d2) and every
  descendant of it, which the resolution of the property kind finds. Any other
  kind is not judged. Adding a kind is one line.
- **An extension direction is unverifiable, unless the caller declares it.** The
  rule never guesses "vertical" from the text of a value. A caller that knows
  which sibling is vertical gives a `DeclaredDirection` to
  `directional_ratio_declaring`; the declaration serves that one run and is never
  written into the package.
- **A check that cannot look says so**: no array reader, a kind that does not
  resolve, arrays that do not read or of different lengths, no pair of finite
  values: each is an *unverifiable* finding on the property concerned, never a
  silent pass.

The physical reference for the expectation is not a document of the registry's
closed list. TODO(verify): a petrophysics handbook to cite, with its page, for the
anisotropy of a layered medium.

## Consequences

- A package of an array resistivity logged in two directions can now carry a
  warning in the report of a check, and the view of a property can run the rule
  again with a person's declaration.
- A package whose directions are written with a producer's prefix says
  *unverifiable* until somebody declares them. That is more noise than a guess
  and less than a wrong answer.
- A threshold the caller wants otherwise is not a parameter: the rule is one
  statement of the registry, and a changed threshold is a changed rule.
- The text of the rule in `energyml-rules/rules/property.toml` and the constants
  here are two sources of one fact; the numbers of the text are the code's.
