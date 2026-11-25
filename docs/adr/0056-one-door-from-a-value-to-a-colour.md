# 0056 - One door from a value to a colour

Status: accepted. Adds `energyml_domain::color::ValueColours`; changes nothing of what a file's colour map says. Completes
[ADR-0050](0050-the-colour-of-a-category-is-carried-by-its-table.md) and [ADR-0054](0054-the-colours-along-a-well-come-through-one-door.md).

## Context

What colour a value takes was decided in four places, each answering the same three questions: does the file give a colour (a map,
a constant)? is the value a category, read by its exact key, or a measure, placed on a scale? what stands in when the file says
nothing, and where does its scale come from? The views (`energyml-views::colour`), the units of a wellbore (`well_strata`), the legend of a
scene (`energyml-geometry::scene`) and the 3D of the desktop (`energyml_paint::look`) drifted: a category with no entry in a
discrete map took the colour of its lower neighbour in one and no colour in another.

RESQML TRG 2.2 says, of a `DiscreteColorMap` (section 4.11, p. 62), that it associates an integer to a colour, and states a rule only for a
**continuous** property read through it: the next lowest integer of the map. Of a code with no entry it says nothing. Of a
`ColorInformation` (section 4.5, p. 56) it says that `MinMax` is the range of values that give the map's extreme colours, and that values
outside keep them; absent, it says nothing (ADR 0025: the map's own range of indices stands for it).

## Decision

One type, in `energyml-domain::color`, which is pure (no storage: the studio compiles it to wasm):

- `ValueColours::new(file: Option<&Paint>, reading: Reading, stand_in: Option<&Palette>, extent: Option<(f64, f64)>, keys: &[f64], scaling: Option<Scaling>) -> Result<ValueColours, ColourRefusal>`,
  `.colour(Option<f64>) -> Option<Rgba>`, `.sample(f64) -> Option<[u8; 4]>`, `.source() -> &ColourSource` (also the public field `source`).
- A constant colours every value, a missing one too. A discrete map of the file read as a category (`Reading::Categorical`) is read **by
  exact key** (`ColorMap::sample_key`); a key it has no entry for takes the palette laid over the keys (`Palette::over_categories`: one
  colour per key, in turn), and `ColourSource::FileAndPalette` says it is not the file's; with no palette it has none. Any other map of the
  file is a measure (`ColorMap::sample`) on the file's scale. A palette stands in when the file gives nothing (`ColourSource::Palette`):
  a category palette over the keys, a continuous one over the extent the caller gives - **the extent never reaches a file's map**.
  A missing value takes `NullColor`, or nothing.
- `scaling` is a scale the caller has merged itself (the request's fields over the file's, field by field); it replaces the file's.
- What goes through it: `energyml-views::colour` (`value_colours` and `category_colours` ask the package's colours and call it; `ColourSource` and
  `ValueColours` are re-exported there), the choice of `by_key` in `well_strata`, the ramp of the scene legend in `energyml-geometry`.
  The studio and the desktop's `energyml_paint::look` call it too, instead of `ColorMap::sample` and their own palette over categories.

## Consequences

- A category with no entry in the file's map has **no colour from the map** everywhere, and the desktop adopts the exact key (its 3D read
  the nearest lower entry, which ADR-0050 says is not for a category).
- `ColourSource` loses its `Serialize` derive (it was serialised nowhere); it moved with its type.
- The notes of the colours (a hue read in turns, ADR 0055) are the cascade's and the door's callers' and are unchanged.
