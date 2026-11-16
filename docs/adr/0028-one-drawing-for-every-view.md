# 0028 - One drawing for every view

Status: accepted

## Context

Several views were specified in parallel, and each came with its own display list:
a box, path and text list for the table schema and the provenance graph, a scene
for the target view, a layout with an image shape for the cross-section, panels
and layers for the composed views, figures for the statistics. The cross-section
already has a list of its own (`energyml-section`), written out as SVG and read
attribute by attribute by its script, the web studio and the desktop. Three
rules for graduations exist, in the cross-section, in the studio and in the
desktop, and they disagree (the desktop picks the round step nearest to the
ideal one, the others the smallest one that gives no more than the target). A
workflow engine will later need the same layered graph placement as the
provenance graph.

## Decision

- **One module, `energyml_sheet::draw`, and it knows no format.** It names no
  EnergyML crate (core, domain, storage) and no module of `energyml-sheet`
  outside itself (`crate::` appears only as `crate::draw`). A test holds it
  (`tests/draw/no_format.rs`), so that the module can be moved into a crate of
  its own without a rewrite.
- **One `DisplayList`.** Keyed elements in ordered layers, in pixels or
  millimetres, y down. Its SVG writer, its hit test and its PNG all read this one
  value. A coordinate that is not a finite number is refused by name, never
  written as 0.
- **One rule for graduations.** `draw::ticks`: a step of 1, 2 or 5 times a power
  of ten, the smallest one that gives no more than the target number of
  intervals, exact as a decimal; logarithmic axes in three densities; refusals
  by name. The cross-section calls it; the desktop and the studio will call it.
- **A colour is a closure of the caller.** Rasters are painted through a function
  the caller passes; `draw` reads no colour map, so the one door to colours stays
  with the caller.
- **A PNG is always the SVG rasterised.** The only way to a PNG is
  `png::render(svg)`, by `resvg` with its `raster-images` feature and an image
  resolver that accepts only a PNG embedded in a `data:` URL. The other ways were
  set aside: a finer feature does not exist; an `ImageHrefResolver` without
  `raster-images` can only hand back vectors, one rectangle per pixel; drawing
  the images ourselves would be a second renderer. The feature adds eight
  pure-Rust packages to `Cargo.lock` and three decoders (GIF, WebP, JPEG) that
  are linked and never reached. The `png` feature is off by default: without it,
  a call refuses by name.

## Consequences

- The cross-section migrates to `draw` later, in a lot of its own: its nested
  groups, its zoom attributes, its layers for Inkscape and its byte-for-byte
  tests need more than `DisplayList` has today. Until then, three second
  implementations remain, and are known: the XML escaping in `energyml-section`
  and in the sheet's SVG writer, and the section's own PNG renderer.
- The font exists twice (`energyml-section/fonts` and `energyml-sheet/fonts`),
  because a published crate cannot embed a file of another crate. The width
  table stays where its generator writes it.
- The desktop, which drew fewer graduations on some ranges, will draw the
  cross-section's. This visible change is intended.
