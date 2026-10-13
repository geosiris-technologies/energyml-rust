# energyml-section — architecture

Related decisions: [ADR 0015](../../docs/adr/0015-section-view.md) (the view: three stages, `model.rs` as contract, one drawer), [ADR 0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR 0011](../../docs/adr/0011-tests-follow-the-specification.md), [ADR 0012](../../docs/adr/0012-no-face-carries-logic.md).

## Purpose and position

The section view: wells projected and surfaces cut along a trace, as long profiles and cross profiles, drawn as SVG (optionally PNG, behind the `png` feature). It sits above geometry and the package index, below every face. (The directory is `energyml-section`; the published name may differ, see the repository README.)

## Three stages, three costs

`read` (per package, the only stage that reads) → `assemble` → `Catalogue` (cacheable); then `layout` (catalogue + `View`) → `svg::write`. Faces call `compute` and `draw`, never the stages one by one. Changing a **view** never rebuilds a catalogue; changing an **input** (`Inputs`) does.

## Rules

- **`model.rs` is the contract.** Every module is written against its types, never against another module's internals. They travel as camelCase JSON (to a UI, in the SVG metadata, and back). A new field is added there, with its reason.
- **One drawer.** Screen, download, CLI and package attachment all go through `draw` with the same view: the export is what was shown. A second drawing path would reopen the duplication this removed.
- **The catalogue holds only what the file says.** Colours of the scene are brought by the view (`View.colours`, `View::paints`), never stored in the catalogue.
- **Every refusal is one line** (`ObjectRow`), in English, naming the object. An object that crosses nothing is `Missed`, not a refusal.
- **Nothing that is not a number leaves a contribution** (`read::finite`). `serde_json` writes `NaN` as `null`; the catalogue used to refuse a whole package for one well with no elevation. A non-finite head, or fewer than two finite nodes, is a named refusal; a dropped node or interval of a kept well is a `notANumber` note, never a refusal (two lines for one object). A surface cut breaks at a hole and is never joined across it. Every reader goes through `contribution_cached`, hence through this guard.
- **The head alone decides that a well is kept**; its nodes are placed as `s + (node − head)·direction`.
- **One borehole, one line.** A frame and a trajectory of the same wellbore merge, the frame being kept.
- **No merging at a distance.** `read::contribution_bounded` reads only wells whose head falls in a window of a corridor, and the whole borehole as soon as one member is near. A borehole with every member far is neither read nor merged; it is refused on its own row ("farther than N…"). The drawing is identical.
- **Two crossing traverses are noted, never refused**; at a vertex a traverse follows the mean bisector, like the well projection.
- **The SVG is hostile-input safe.** Colours as attributes, all text escaped (a package title is someone else's text), one `<g>` per object with `data-store` and `data-uri`. The page key contains a NUL, forbidden in XML, so `data-key` carries it as U+2400.
- **The frame keeps half the drawing.** `layout::budget` shares the height: the plot keeps `PLOT_SHARE`; notes fold first (down to one line that counts the others, never silently dropped), then the names band. Marks go through a `Placer` and never overlap; a mark without room keeps its stroke, loses its text, and is counted.
- **A fill runs to the next horizon, abscissa by abscissa** (`fill::is_candidate`, `fill::polygons`, shared by drawing and panels): the neighbour is chosen at each abscissa (nearest strictly above or below); nothing is filled where there is none (no invented floor); a fault is never a candidate; two surfaces interpreting the same feature are not filled toward each other (a horizon drawn twice). Who gets a fill button is decided here (`ObjectRow.fillable`); a UI reads it.
- **Consistency at crossings is reported** (`crossingMismatch`, `crossingOneSided`, tolerance 0.5 % of the vertical extent, at least a centimetre) and **erosion is detected only from stated ages** (`HasOccurredDuring`, possible ages, absolute age): no age is deduced; a correction is proposed in words and never applied.
- **Declared, not spelled.** Styles for unstratified portions (`style::declared()`), marker shapes (read from the model's `NodeSymbol` enumeration, `markers::declared`) and strata-legend options are declared by the library with their parameters and bounds; a UI builds its form from the list and spells no style. A value the drawer cannot draw is refused by name, never replaced by a default shape. Defaults are not serialised, so existing drawings keep their bytes.
- **Several packages, one cache per package.** `Inputs::objects` holds one `Asked` per package; an object whose package is not opened is refused by name. The per-object cache (`read::cache::PackageCache`) keeps the read well and the placed surface (independent of the trace) and the cuts keyed by (surface, line); the epoch key is package, revision, scene system and time factor. Tests count work (`Work`), never time.
- **A grid is cut as an envelope** with the reader of `energyml-geometry` (never a second reader); what is approximated is written at the head of the module.
- **`redraw` rebuilds a drawing from its own metadata**: same packages, same bytes.

## Not done

No byte budget on the cache; no stratigraphic order inference for erosion beyond stated ages.

## Tests

Single test binary (`tests/main.rs`). Each layout rule has a test that was checked by sabotage; the corridor, finite, fill, marker and style tests are the evidence for the rules above. A drawn SVG is compared byte for byte on the platform that wrote it.
