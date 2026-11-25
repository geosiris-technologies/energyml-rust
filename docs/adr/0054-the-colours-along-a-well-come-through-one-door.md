# 0054 - The colours along a borehole come through one door: a property by its title, carried by measured depth

Status: accepted. Adds a function to `energyml-views`, a model piece to `energyml-section` and an option to `energyml profile`;
builds on [ADR-0049](0049-the-values-of-a-property-along-a-frame.md),
[ADR-0050](0050-the-colour-of-a-category-is-carried-by-its-table.md) and
[ADR-0052](0052-limits-across-a-well-and-the-ramp-legend-of-a-section.md).

## Context

A section keeps a well as a `WellItem`: one object (a frame, or a trajectory), its nodes in `(s, z)` and the measured depth
of each. A person who wants to see a property along it - the strata a log gives, a porosity - rarely finds that property on
the object the section kept: it hangs on **another frame of the same well**. The section view takes the colours it paints
(`View::paints`) from its caller, "resolved by the front end", so each face had to find the property, carry its values onto
the nodes and colour them, and the command line had no way to do it at all.

## Decision

- **One door, in `energyml-views`: `paint_wells`**, beside `value_colours`, whose context (`ViewContext`: the objects,
  the index, the package's colours, the palettes) it already takes. It is not in `energyml-section` because the section
  draws what it is given and reads no property; it is not in the command line because no face holds logic. The desktop
  and the command line ask the same function and apply its answer to their `View` (`WellPaints::apply`).
- **The property is chosen by its title**: a piece of it, case ignored - `energyml_storage::resource::title_matches`, the
  reading `list`, `show` and the markers share (the library has no regular-expression title filter, and a second reading of a
  title is a second answer). It is looked for among the properties of **the well the item stands for** (the item's object
  and the objects folded into it, found in `well_members_of_index`) that hang on one of the well's frames.
  - none fits: the well keeps its colour and is **counted** (`WellOutcome::WithoutProperty`);
  - several properties of **different titles** fit: the well is **refused, naming them**; no property is chosen between
    them. The same title on several frames of the well is one property in parts, and two of that title on one frame is a
    refusal that names both;
  - a measure on one frame and a category on another, a property that does not read, MDs that cannot be compared with the
    well's (two trajectories, no common datum - the rule of `strata_by_md::comparable`): refused, by name.
- **The values are carried by MD** onto the nodes the section kept, with `frame_values::carry` (ADR-0049): a node whose MD
  no frame holds, or where two frames disagree, has **no colour**, a gap and never a neighbour's value. A section that keeps
  no MD for its nodes cannot be painted this way, and says so.
- **The colours are the cascade's.** A category goes through `value_colours` with the property's uri: the colour map put on
  its table (ADR-0050) read **by exact key**, a palette for a key the map lacks, and the answer says which one painted.
  [`Blend::Net`] for a category, [`Blend::Smooth`] for a measure, whose scale runs from the lowest value painted to the highest,
  over every well of the call (not the percentiles an image takes: no node loses its colour to a clamp).
  One call paints one reading: the first well painted says whether it is a category or a measure.
- **A palette that stands in for a file's map is laid over the keys, not stretched over bounds**
  (`energyml_views::category_colours`, from `Palette::over_categories`): the colours in turn by the rank of the key among
  the rows of the table and the codes painted. The first version asked `value_colours`, which lays a ramp between two bounds:
  with the bounds of a category (0 and 1) the first code took the first colour and every other the last. Colour of a key the
  file's map lacks is the palette's, and the source says it.
- **A colour information may name its map by `DiscreteColorMap` or `ContinuousColorMap`**, the elements of the draft of
  RESQML 2.2 (`v2.2_dev3`), as well as by `ColorMap`, the released schema's. A package written against the draft (the
  colour of the strata of a borehole set) named its map that way and every entry was read as naming none, so the cascade
  found nothing on the table and the palette painted. The three names hold one reference. `ValueVectorIndex` is unchanged:
  the guides say it indexes the components of a vector property and are silent on a table, so the rule of ADR-0050 (a
  column of the table, the key column being 0, refused when it does not exist) stands, and here column 1 is the `Column`
  whose values the map's entries 0 to 4 - the keys - colour.
- **The wells that carry a paint are drawn after the others**: the frames of a wellbore lie at one place, and a frame drawn
  in its own colour hid the one that carried the paint. No well is dropped; the grouping of the wells of a wellbore is
  another question.
- **The wells the scene paints nothing along keep their strata**: `View::strata_for_unpainted` (off by default and not written
  when off, so older drawings and their metadata keep their bytes) makes the wells absent from `View::paints` take the
  strata branch under `WellColouring::Scene` - intervals, ticks, portions with no strata, and the strata legend - one decision,
  `strata_for(view, well)`. `WellPaints::apply` and `--paint-wells` set it. A further `WellColouring` variant (scene over strata)
  was the alternative: it names the combination in one value, but it adds a case every consumer of the enumeration has to handle
  and that an older reader of the metadata cannot parse, where a flag that is absent when off costs nothing to either.
- **A stretch with no value is not an absent well**: a node the property gives no value to (outside the frames that hold it, or a
  null) keeps the line of a well with no paint - the colour of the view's unpainted wells, or its strata where `strata_for_unpainted` is
  set and the well has them. It is `layout.rs` that lays that line under the pieces of the paint (`paint::gaps` names the stretches),
  rather than the view's door putting a colour in the paint: the paint stays what the scene shows, a node without a colour, and the
  drawing - which alone knows what an unpainted well looks like in this view - decides what a hole looks like. A well refused for
  having no value at any node says by cause and counts each: the nodes outside every frame, the nodes with a null value.
- **The legend is named by what the wells share**: the title their property kind goes by when they agree (`[Strati] code`),
  else the title their properties all bear, else the piece of title that was asked; the notes that every well repeats (where the colours come from) are said once with
  their count.
- **The legend is the section's**: a ramp (`View::ramp_legend`, ADR-0052) for a measure, and for a category a new
  `View::category_legend` - the value (its label when the table gives one, its code otherwise) and the colour, one object per
  category, in a group of its own (`category-legend`), under the other legends on the right. Neither is written into the
  metadata when absent, so a drawing without them keeps its bytes.
- **The strata lines stay over the paint**: `energyml_section::limits::strata_limits` turns the units a well shows into limits
  (the tops, named by the titles of the unit above and below, through `energyml_domain::limits`: a unit that comes back is
  two runs, nothing crosses a hole), which `View::limits` draws over the colours of any well (ADR-0052).
- **`energyml profile --svg --paint-wells TITLE`** gathers the wells of the section drawn, asks the door once per package, puts
  the answer in the view and prints what became of each well: painted, without the property (counted), refused (named). With
  `--strata-ticks`, the strata lines are laid over the paint. Only for a section; `--draw plan` is refused.

## Alternatives considered

- **A regular expression for the title (`/regex/`).** The library has none, and the expression would be a second reading of a
  title beside `title_matches`; the piece of the title is what the other commands take.
- **Taking the first property that fits.** A silent choice: a person who painted by the wrong property would not know.
- **The percentiles of the values as the scale.** They are an image's rule, and would leave the extremes of a well without a
  colour; a well is a few hundred nodes and the extremes are the point.
- **Colours per well for a measure.** Two wells with two maps would draw one legend that is true of one of them: one scale and
  the first property's colours for every well.

## Consequences

- The colours the command line paints along a well are those the desktop will paint: nothing of the finding, the carrying or the
  colouring is in either.
- `energyml profile` turns the colour layers of the cache on (`layers::COMMANDS`), for the package's own colours.
- `View` gains `category_legend`, absent by default and not written: an older drawing reads back unchanged.
