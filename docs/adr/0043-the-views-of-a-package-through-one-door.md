# 0043 - The views of a package are drawn through one door, as a display list written to SVG

Status: accepted

## Context

The library computes much of what a view of a package shows: a well's line laid flat and the depth on it, the
values of a property along a frame, the graph of the activities that wrote the package, the links between tables.
Only one view went as far as a picture (the geosteering section, `energyml_section::along_well`), and no command
drew it. A command line, a desktop window and a web page each need the same pictures; drawn in each face, a picture
would be drawn three ways, and the SVG exported by one would not be what another shows.

## Decision

- **A crate of its own, `energyml-views`**, above the crates that compute (domain, geometry, cache, sheet's
  drawing, section): nothing below it depends on it, so no edge is added between the crates that compute.
- **One registry of views** (`ViewKinds`, the caller's first): each view declares the objects it takes (slots of
  types, by their name without release), the settings it reads (nature, bounds, default, sentence), what it can show
  in a package from the index alone (`offers`), and draws a `DisplayList` from what the other crates compute. A view
  lays out; it does not compute a second time.
- **One door** (`draw_svg`): the request (`ViewAsk`) is checked against the view's declarations - an object the
  package does not hold, of a type no slot takes, a setting not declared, out of its bounds or of another nature,
  each is refused by name - drawn, and written as SVG with the request, **completed with its defaults**, in the
  metadata. `redraw` reads it back and draws again: the same bytes over the same package. A face that draws the list
  itself asks `draw_list`.
- **A PNG is the SVG rasterised** (`png`, behind the `png` feature), never a second drawing.
- **Colours by one door**: the map the package gives the object (`energyml_cache::colours`, the walk `energyml
  colors` prints), else a palette of `energyml_domain::palettes`, named in the legend as not from the file; a palette
  the request names wins. No colour is invented: a value the map has none for stays transparent.
- **The command line** asks the same door: `energyml view list | about | draw | redraw`, `--svg` written, `--png`
  on demand. It holds no logic.

## Consequences

- A view is one file and one registry line; a face gains it without a line of its own.
- The settings of a view resemble the slots of an operation of `energyml-build` without their provenance; they are a
  type of their own here, so that `energyml-views` does not depend on `energyml-build`. Bringing the two together is
  left for later.
- The well sheet of `energyml-sheet` and the geosteering section keep their computing; their pictures move to the
  display list and the views' metadata.
