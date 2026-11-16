# 0037 - A result of a view is an artefact first, and is stored on demand

Status: accepted

## Context

The views of the workspace compute things that are not objects of the package:
a geosteering section drawn along a well, an iso-surface cut in a cube of
resistivity, a trace with its cross sections. A view shows its result at once, and
most of the time nobody wants it in the package: writing every drawing would fill
a package with files nobody asked for. Yet some results have to be kept, with the
recipe that made them, so that the same drawing can be found again and the
objects it was computed from are known.

Two facts make the recipe delicate:

- **An operation is a pure function.** A plan reads no array and no store
  (ADR-0017). A cube of two million values, the positions of a lattice placed
  along a trajectory, the frame of a hole: none of that is in the bag an
  operation receives, so an operation cannot compute an iso-surface again.
- **The template of an activity declares each of its parameters**
  (common TRG 2.3 section 3.1 p. 40, ADR-0035). A generic template, one for
  every view, cannot say what each view was told.

## Decision

- **The result is computed and drawn as an artefact, and promoted when somebody
  asks.** The computation is the one of the library (`energyml_geostat::isosurface`,
  `energyml_section::along_well`); the scene keeps the artefact; an operation of
  `energyml-build` stores it on a gesture, never by itself.
- **One operation per sort of view result.** `along-well-view` stores a
  geosteering section, `isosurface` an iso-surface; the section view and the
  coverage follow the same model. Each has its own template, so that each
  parameter is declared. A view with no operation of its own is not stored.
- **The drawing is the input, and the recipe is read from it.** The `<metadata>` of
  the SVG holds the request and the view; the operation reads them back (the one
  reader, `svg::read_metadata`), and a setting given beside the drawing is held to
  it: a disagreement is refused with both values. A second statement of the recipe
  would be free to disagree with the first.
- **The objects the drawing names must be held**, compared on the full uri. One that
  the package lacks is refused by its uri: the drawing was made from another
  package, or before that object was written.
- **The iso-surface is promoted as it was computed.** The operation takes the mesh
  and the declared recipe (level, spacing, method, the cells set aside, the inputs)
  and writes the mesh through the writer of `triangles`, the one writer of
  triangulated sets. It does **not** recompute, and the note `promoted` says so. A
  lattice that folds on itself is refused by the caller **before** it computes; the
  operation cannot check it and does not pretend to.
- **The images are attached to the frame of the section**, in the part-address form
  of `attach`, and named by the frame and what they are, so that drawing a section
  again replaces its own files.
- **Stored views are read by the template of their activity, never by its title**
  (`read::stored_views`). The loop that finds the activities is one function, shared
  with the stored sections.
- **A PNG is the PNG of the SVG that is stored with it**, made by
  `draw::png::render` from that SVG (ADR-0028). The operation checks that it is a PNG
  by its signature and that it has the proportions of the drawing; it draws nothing.

## Consequences

- A package can carry the recipe of a drawing without the drawing being an object
  of its own: the activity names the inputs, the part holds the SVG.
- The recipe of a promoted iso-surface is as true as the caller's declaration. A
  caller that declares a level it did not use writes a false activity, and nothing
  in a pure operation can catch it. That is a limit stated here, not hidden; a check
  that recomputes belongs to a face that holds the cube.
- A new sort of view result is an operation and a template, not a parameter of an
  existing one. That is more code than a generic store, and an activity a reader can
  understand.
- Two doors to a triangulated set (`triangles` and `isosurface`) write the same bytes
  for the same mesh and naming; a test compares them.
