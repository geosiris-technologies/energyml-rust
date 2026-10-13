# 0015 - The section view: three stages, `model.rs` as contract, one drawer

Status: accepted

## Context

A section along a trace draws wells, surfaces and grids from one or several
packages. Reading and cutting are expensive; drawing, scaling and labelling are
cheap and change on every interaction. A drawing computed in several places
diverges.

## Decision

- **Three stages with three costs.** `read` (per package; the only stage that
  touches a store), `assemble`, then a `Catalogue` that can be cached; then
  `layout` (catalogue plus a `View`) and `svg::write`. Changing the view never
  rebuilds a catalogue; changing the inputs does.
- **`model.rs` is the contract.** Every module is written against its types, which
  travel as JSON (also embedded in the SVG metadata). A new field is added there,
  with its reason.
- **One drawer.** Screen, download, command line and package export go through
  the same `draw` with the same view: the export is what was shown.
- Nothing about scene colours lives in the catalogue; the view supplies them.
- Every refusal is a line naming the object; a non-finite number never leaves a
  contribution; the SVG escapes all text, carries colours as attributes and keys
  each object for the interface.
- The drawing can be recomputed from its own metadata, giving identical bytes on
  the same packages.

## Consequences

- Views are cheap to change; large packages are cut once per line.
- The same geometry and colour rules apply everywhere.
- The crate has no native dependency and builds for WebAssembly.

See [Section view](../section-view.md).
