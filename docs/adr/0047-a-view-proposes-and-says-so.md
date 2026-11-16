# 0047 - A view may propose a value the file does not state, and says so

Status: accepted

## Context

Some views need a value the file never states: the roles of the two dimensions of a curtain, the table that holds its
coordinates. Following [ADR 0005](0005-refuse-rather-than-draw.md) and [ADR 0041](0041-declared-conventions-and-the-vertical-without-crs.md),
a request that does not give them is refused. That is right for a recipe, and unhelpful to a person who opens a
view for the first time: a refusal shows nothing, and nothing says what to give.

## Decision

- **A view can propose** (`ViewKind::assist`, reached by `energyml_views::assist`): values for settings the file
  does not state, each with the reason it is proposed (`Proposal`), and the dimensions it needs declared with the
  candidates the package offers for each (`DimensionAxes`, `AxisColumn`). It reads the package index and the shape
  of the tables only, never an array.
- **A proposal is not a reading.** It is applied only to a setting the request does not give, only by a face that
  asked for it (`energyml view draw --proposed`, a form's first drawing), and it is said wherever it shows: a note on the
  command line, a mark and the reason in a form. A value the person gives replaces it.
- **Several candidates are a choice, never a guess.** A table is proposed for the links of a curtain only when
  it is the one table that fits (one key column, one numeric value column per dimension, the right rows); with two,
  both are candidates and nothing is proposed. The roles are a declaration, proposed as such.
- **The default of an index is not a proposal.** `lateral_index`, left out, is the middle row of the lateral
  dimension, and the help says so; row 0, the edge of the cube, was off the plane of the hole.
- **The wire shape of the links has one writer** (`axis_links_value`) and one reader (`axis_links_read`), so no
  face writes the JSON itself.

## Consequences

- A first drawing exists for a package with one table of axes, and what is proposed is visible and replaceable.
- A package with several tables never gets a silent choice.
- A view that has nothing to propose costs nothing: the default method answers an empty `Assist`.
