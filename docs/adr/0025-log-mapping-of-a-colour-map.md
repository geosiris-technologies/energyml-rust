# 0025 - A logarithmic mapping places the log of the value

Status: accepted

## Context

Two things in a colour map are called logarithmic, and the code treated them as
one: `ColorInformation.UseLogarithmicMapping` and
`ContinuousColorMap.InterpolationMethod = logarithmic`. The reading was "either one
turns on a logarithmic spacing between the two stops around the value". For a map
whose first index is 0, that flattened the colours: the lower bound was lifted to the
smallest positive number, and every value fell near the top of the range.

The Technical Reference separates them (RESQML TRG 2.2):

- `UseLogarithmicMapping` "indicates that the log of the property values are taken
  into account when mapped with the index of the color map" (section 4.5, p. 56).
  `MinMax` is "the range of values of the associated property which will result in
  the minimum color and the maximum color", and a value outside it keeps the extreme
  color (same page).
- `InterpolationMethod` is "the method for the interpolation between color map
  entries", `linear` or `logarithmic` (section 4.7, p. 58 and section 4.23, p. 74).

The standard gives no base for the logarithm, and does not say what the range of the
values is when `MinMax` is absent.

## Decision

- **`UseLogarithmicMapping` places the log of the value** between the logs of the
  range, then the map is read as it is written. The base does not matter: placing a
  log between the logs of two bounds needs none.
- **The range is `MinMax`, or else the map's own range** (its first and last index).
  This is the stand-in the linear mapping already uses, where placing a value on the
  map's range is the identity. The other reading, that the index of the map is the
  log of the value, needs a base the standard does not give: it is not taken.
- **`InterpolationMethod` stays the spacing between two stops** and nothing else. The
  two can be combined.
- **A range that reaches zero or less is refused by name.** `ColorMap::check_scaling`
  says why, once, for every value; `try_sample` returns the refusal; `sample` keeps
  its signature and answers `None`. A missing value keeps its null colour, before any
  refusal. A value outside `MinMax` is clamped before the log, so a value of zero or
  less takes the extreme colour, never a `NaN`.
- **A house rule records it**, `house/colour/log-mapping-is-log-of-value`, with its
  references.

## Consequences

- Every map that uses `UseLogarithmicMapping` changes colours: this is the fix.
- A logarithmic map with no `MinMax` whose first index is 0 or less no longer
  colours. A caller asks `check_scaling` once and shows the reason; this is
  [ADR 0005](0005-refuse-rather-than-draw.md).
- The writer already refuses a `MinMax` of zero or less; it does not yet know the
  map's indices when `MinMax` is absent, and may write a map the reader now refuses.
