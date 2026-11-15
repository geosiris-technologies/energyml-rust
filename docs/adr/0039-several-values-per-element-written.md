# 0039 - Several values per element are written with their shape

Status: accepted

## Context

A property of the package can hold more than one value per element: a matrix per
node (the cube of an azimuthal resistivity tool holds `[81, 81]` per station), a
vector, a tensor. The standard says it in `ValueCountPerIndexableElement`, a list
of counts, slowest first, and in `LabelPerComponent` for the names of the
components (RESQML TRG 2.2 section 7.1 p. 222). The note of the figure p. 221 gives
the order of the dataset: the elements first, then the dimensions of one element, and
time slowest of all when the property is a series.

Until now the one writer of a `ContinuousProperty` wrote `[1]` and a flat array.
A property derived from such a cube (the ratio of two directions, the difference of
two passes) would have lost its shape, and a reader would have taken a matrix per
node for a long scalar column.

## Decision

- **`property::write_shaped` is the one writer**, and `write_timed` is a call to it
  with a scalar shape. There is no second function that writes a property.
- **The shape is the one of the domain** (`ElementShape`): its dimensions are written
  as `ValueCountPerIndexableElement`, its labels, when it has some, as
  `LabelPerComponent`; without labels the field is absent, never an empty list.
- **The dataset is `[elements, d1, d2, ...]`**, or `[steps, elements, d1, ...]` for a
  series: time the slowest dimension, the elements, then the shape of one element.
- **A scalar writes what it always wrote**: `[1]` and a flat array, the same bytes
  as before. A test holds it.
- **The nodes are counted in elements**, not in values: a representation of three
  nodes does not take eight values of a shape `[2, 2]`, and the refusal names both
  counts. A number of values that is not a whole number of elements is refused, never
  truncated.
- **`CountPerValue` stays 1.** The common TRG 2.3 does not define it for a floating
  point array (section 6.19 p. 95 names the attribute for `BooleanExternalArray`
  without a note); the shape is said by `ValueCountPerIndexableElement` and by the
  dimensions of the dataset. TODO(verify): a statement of the standard on
  `CountPerValue` of a floating point external array.
- **The shape is not a parameter of the activity.** It is in the object written, the
  template of `property` declares no slot of shape. The operation `combine`, which
  writes a derived property, asks for the shape of its parents (`valuesPerElement` and
  `componentLabels`) because a plan reads no object but the ones it is given.

## Consequences

- A derived property keeps the shape of its parents, and the readers that already
  understand `ValueCountPerIndexableElement` read it back without a change.
- A caller that states a shape different from the parents' writes a property of that
  shape: the operation checks the number of values against it, not against the
  parents' own declaration, which it is not given.
- The question of `CountPerValue` stays open until the standard answers it; the
  value 1 is the one every writer of the workspace has written.
