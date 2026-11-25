# 0052 - The lines across a well at its limits, and the ramp legend of a section

Status: accepted. Extends [ADR-0015](0015-section-view.md); the limits come from [ADR-0051](0051-the-limits-between-two-runs-of-values.md).

## Context

A section drew one kind of line across a well: the **tick** at the boundary between two strata, black, one pixel,
written in the code, and only for a well coloured by its strata (a well the scene paints lost its strata and its ticks
with them). Meanwhile `energyml_domain::limits` finds, for any suite along a well (units, codes, a table column, a
threshold on a measure), every limit between two runs, with its rank and the classes it separates, and places it in MD.
Drawing those limits - where a pressure crosses 6 bar, where a facies changes - meant a second kind of line, and a second
reading of "a line across a well".

The section also had a legend for strata and one for markers, and no legend for the colours of a property; the view along
a well has one, private to its layout.

## Decision

- **One geometry, one style, one writer.** The line across a well at a point of its path is `tick_through` (perpendicular to
  the path, centred on it, as wide as the well and two pixels more); the strata tick is `tick_through` at the start of an
  interval, a limit is `tick_through` where the limit lies. The colour and the width of every such line are
  `View::limit_style`, whose default is the black line of one pixel the ticks always were - then not written into the
  metadata, and the bytes of a drawing that asks nothing do not change. A style that cannot be drawn (not a hexadecimal
  colour, a width outside `(0, 40]`) is said in the drawing's notes and the default is drawn.
- **Limits are data of the view.** `View::limits` gives each well (by key) its `WellLimit`s: where (`LimitAt::Md`, or a node
  of the path and a fraction), its rank among the limits of its class, and the names of what lies above and below.
  `limits::well_limits(transitions, kind, anchor)` makes them from the domain's `Transitions`: the tops or the bases,
  placed by the anchor asked; a limit the domain cannot place (an interpolation over a hole) is left out and said, never
  drawn where it is not. A limit that is not on the well's path (an MD outside its MDs, a node it does not have) is not
  drawn and is counted in a note.
- **They are drawn over what the well draws** - its strata, its own line, and the pieces of a paint the scene lays along
  it - in a group of their own, `<g data-part="limits">`, each line saying its rank, its MD and what it separates
  (`data-limit-*`). Under scene colouring the well still has no strata and no strata ticks (the scene's paint is what is
  shown); the limits are what the paint does not say, and the decision is to draw them over it.
- **One ramp legend per section** (`View::ramp_legend`): the property's name and unit, the ramp from its lower bound at the
  bottom to its upper at the top, graduated. This crate decides no colour: the caller samples the ramp
  (`ramp::sampled(title, uom, scale, colour)`, a closure from a value to a colour - `energyml-views`' `value_colours`, a
  palette of `energyml_domain::palettes`), and the legend carries the colours it was given. The steps and the graduations are
  `ramp::STEPS`, `ramp::value_at_step` and `ramp::graduations`, **shared with the legend of the view along a well**, which
  now calls them: one arithmetic for both. A ramp that cannot be graduated (equal or reversed bounds, no colour) is said and
  not drawn, and takes no room. It is an Inkscape layer of its own (`ramp-legend`), stacked on the right under the legends of
  codes and markers.
- The stale comment of `cut_sources` is corrected: `TraceItem` carries `sources`.

## Alternatives considered

- **A tick per limit in `LaidInterval`.** A limit is not an interval's: it lies at an MD of the well, with a rank and a pair
  of classes, and a painted well has no intervals of strata to hang it on.
- **A ramp legend that takes a colour map.** A map is not in the view's data, the view travels as JSON into the SVG's
  metadata, and the colour is the caller's decision (ADR-0043).
- **Letting the strata ticks keep their own colour.** Two lines across a well with two styles is what the drawing then
  shows when both are asked.

## Consequences

- A drawing that asks for none of this is the same bytes; `LaidObject` gains `limits` (empty for everything but a well).
- The desktop application and the studio put the domain's limits on a section by `well_limits` and a ramp by
  `ramp::sampled`; the SVG is the one output, and the PNG its rasterisation.
- The contacts operation and the strata line of the section read the same limits (ADR-0051).
