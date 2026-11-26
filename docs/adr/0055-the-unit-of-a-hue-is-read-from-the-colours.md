# 0055 - The unit of a hue is read from the colours it belongs to

Status: accepted. Adds `energyml_core::color`; changes how `energyml-domain` reads an `HsvColor`; builds on [ADR-0009](0009-properties-tables-colours.md).

## Context

RESQML TRG 2.2 section 4.21 (`HsvColor`) says of `Hue` only *"Hue of the color in the HSV model"*, with a link to Wikipedia: **no unit, no bound**. The library read it in degrees, 0-360, and refused anything outside. A real package (the strata of a metro line, written by GDM Multilayer) writes `0.602`, `0.167`, `0.922`, `0.916` and `0` for its five strata: fractions of a turn, a blue, a yellow, two pinks and a red. Read in degrees they are five reds.

## Decision

- **The unit is read from the values**, by one rule, in one pure function: `energyml_core::color::hue_unit(hues: &[f64]) -> HueUnit`, with `to_degrees(hue, unit)`. `energyml-core` holds it because it is the one crate every reader and writer of numbers reaches without a store (wasm included): `energyml-domain` reads the objects with it, and the translator, which writes the hues as fractions of a turn, applies the same rule.
- **By a set of colours, never by one colour alone.** The set is all the hues of one colour map (its entries and its null, above and below colours), or, for a constant colour, all the constant colours of the same `GraphicalInformationSet`. The hues are **turns** when all of them lie in 0 to 1 and one lies strictly between; **degrees** when any lies above 1 (a hue in `]1, 360]` forces degrees), below 0 or is no number; **undecided** (read as degrees, with no effect) for no hue, or only 0 and 1 - the same red in either unit, to a degree.
- Saturation, value and alpha stay in 0-1 whatever the unit. A hue below 0 or above 360 is still refused by name when it is read (`Hsv::checked`): the rule decides a unit and never a bound.
- **Detect and say.** Every reading in turns gives a note - the object and *hue read as a fraction of a turn: all N hues lie in 0-1* - that goes with the decision (`Decision::notes`), through the colour door of the views (`ColourDoor::paint_noted`) to the notes of a section's paint, and in the notes of the 3D scene as every other note of a decision. The note is a **note**, at the level of the notes (the decision's, the section's, the 3D scene's) and not of the warnings: it says how the map was read. `energyml check` says **nothing** of it.
- **Turns are a valid form, not a defect.** The schema fixes no unit, so both forms are valid, and the translator writes the hues as fractions of a turn (0-1) as the normal form: a translated package carries them by construction. An earlier version of this ADR had `check` warn (`colour-hue-in-turns`); that rule is gone, since it would have flagged every translated package. `Severity::Info` exists but its findings are still listed in the report, and a package in turns must leave `energyml check` with nothing: the rule is removed, not lowered.
- The rare case, said here and in the note: a map written in degrees whose hues are **all under one degree** (all red) is read in turns. The note makes it visible.

## Alternatives considered

- **Degrees only, refusing 0-1 values.** Refuses a package that says something clear to a person.
- **Each colour decided on its own.** A colour of hue 0.5 alone is half a degree or half a turn: it cannot say; a map of five colours does.
- **A reading per producer** (by the `Citation.Format`). A second table to keep, and wrong for the next producer.

## Consequences

- The strata of the package in question come out blue, yellow and pink, with a note on the map.
- A colour map with a single hue strictly between 0 and 1 and nothing else (0.5, 0.5) is read in turns. Its note says it.
- The translator normalises hues to 0-1 by the same function.
