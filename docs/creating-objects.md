# Creating objects

Writing a valid EnergyML object by hand means assembling a uuid, references
(DORs), a CRS, HDF5 paths and activities. `energyml-build` is the layer that makes
writing usable: "wells from this CSV", "a surface from these points", "a map
interpolated from these samples". It has one principle.

> **An operation is a pure function from a bag of pieces to a plan. Only `apply`
> writes.**

```mermaid
graph LR
  inputs["files, objects of a package, choices"] --> pieces["Pieces"]
  pieces --> op["Operation (pure)"]
  op --> plan["Plan: objects, arrays, template, activity"]
  plan -->|quote| report["report: what would be made, what is refused"]
  plan -->|apply| store["EpcStore: the only step that touches storage"]
```

## Why pure

`AnyOperation::plan` takes no store. It cannot read one, so it cannot depend on
one, and a test reaches it with values alone; what the caller knows of the
package arrives already resolved. This gives you, for free:

- **a quote before the invoice**: `--plan` shows the objects, uuids and arrays an
  operation would produce and writes nothing;
- **the same operation behind every face**: the command line, the Python bridge,
  a web page and Galaxy all call the registry; none of them contains a creation
  rule;
- **reproducible output**: uuids are derived (v5) from what the input owns - a
  file and its features, an EPSG code - so a second run over the same source names
  the same objects, and `--now` fixes the instant so the package is identical byte
  for byte.

## A refusal is a sentence

`Registry::offers(&pieces)` answers *what can I create with this?* and lists the
operations that do **not** fit **with their reason** ("a set of polylines is not
written as a triangulated surface"), because *absent* reads as *not written yet*.

### What is refused rather than dropped

An input or a setting a run cannot honour is refused by name, or kept and said,
never ignored:

- a GeoJSON that holds a feature the reader cannot read (null geometry, a
  `GeometryCollection`, a short ring) imports the rest, and each feature left
  out is named in the warnings of `mesh::read_placed`;
- a CSV cloud decides its separator once for the whole file, from its first row
  (a semicolon means decimal commas);
- `interpolate` refuses `faults` with `method = mba`, and `outer` / `inner` with
  any other method; a ring holding several polyline runs is refused (one ring,
  one run);
- `resurface` refuses a grid whose nodes are not on the lattice of its first row
  and column, and keeps a node with no height as a hole (a `holes-kept` note);
- `extend-boreholes` converts `maxDepth` and `step` (metres) into the frame's
  depth unit, and refuses a unit that is no length;
- `attach` refuses `fileName` and `title` with the part form, and `path` with the
  inline and external forms;
- sizes are bounded before anything is allocated: `local-grid` (child nodes),
  `injection-test` (samples), `interpolate` (nodes per axis);
- `transform`, `project`, `clip` and `delete-nodes` place a line or a surface in
  the target system the way they place a point set when `sourceEpsg` differs
  from `project`;
- `sigeol-units` refuses a plan in which no group could be placed, and lists
  the groups it could not place in `dropped`, with no row in the table;
- a borehole with a `BoreholeInterval` whose bound cannot be read is refused by
  `Borehole::mds`, naming the interval;
- `stratigraphy` refuses a parent with only one age bound.

## What an operation declares

| Declaration | Meaning |
|---|---|
| `needs()` | what it still asks the caller: a number (with unit and bounds), an integer, an EPSG code, a flag, a closed choice, text, a CRS, an object of the package... Each is a `Slot` rendered as a CLI option, an interactive question or a form field |
| `takes()` | the pieces it consumes: tables, features, a document, objects of the package |
| `gives()` | the roles of what it writes, with types and cardinality |
| `reports()` | what its activity records that is neither a choice nor an output |

The same declaration generates the command-line help, the web form, the
[Galaxy tool](galaxy.md) and the canonical `ActivityTemplate` of the operation.
Every run records an `Activity` against that template, so the form and the record
are one declaration. Every canonical template declares `input` (the objects the run
read) and `inputFile` (a file it read, written `<slot>: <path>`), and a whole number
a run recorded is an `IntegerQuantityParameter`, as the template says; see
[Provenance](provenance.md).

## The operations

About ninety, grouped by what they do:

| Family | Examples |
|---|---|
| Foundations | crs, collection, representation sets, new version, edit, attach, media, colormap, table-link, along-well-view |
| Import | pointsets and polylines from GeoJSON, wells and properties from CSV/JSON tables (with OSDU column templates), WITSML wells and logs, terrain, GeoSciML GeologicUnit import (tested with SIGEOL exports) |
| Wells | well, well plan, blocked wellbore, computed tops, contacts |
| Surfaces | interpolate (IDW, kriging, MBA), triangulate, isocontours, surface maps (dip, curvature), remesh, densify, extend, clip, loft, outline, intersect, project, measure, thickness, zone statistics, isosurface (an iso-surface kept as a surface) |
| Structural | fault, fault throw, horizon law, boundary, stratigraphy, structural organisation, earth model, cross section |
| Grids and properties | IJK grid, faulted grid, local grid, grid properties, sub-representation, time series, combine (a property derived from two others) |
| PRODML and WITSML | fluid sample, pressure transient analysis, injection test, product flow model |

Run `energyml create --list` for the list your build has, and
`energyml create --list --from your-file.geojson` for what a given input allows.

### Derived properties, links and view results

Four operations write what a computation or a view produced, **only when asked**: the
result is first an artefact (a number, a mesh, a drawing), and promoting it into the
package is a gesture, never a side effect.

- **`combine`** writes a property derived from two others, value by value: a ratio
  (for the anisotropy Rv/Rh of a resistivity), a difference, or the log of a ratio. The
  arithmetic is `energyml_analysis::combine` (see [Geostatistics](geostat.md)); the
  operation adds the writing. The result keeps the shape of its parents (several values
  per element are written with their `ValueCountPerIndexableElement`, and their
  `LabelPerComponent` when they have labels), and its unit is the computed one: a unit
  given against it is refused, never replaced. Two properties on two supports, or of
  two lengths, are refused; a result where every pair is absent or undefined writes
  nothing. The activity records both parents as inputs, the operation, and how many pairs
  were absent and undefined.
- **`isosurface`** promotes an iso-surface computed by `energyml_geostat::isosurface`
  to a `TriangulatedSetRepresentation` with its activity. A plan is pure and reads no
  array, so the operation does not compute the surface again: it writes the mesh **as it
  was computed** and records the recipe (the level, the spacing, the method, how many
  cells were set aside, the cube, its support) **as the caller states it**, and says so in
  a note. The mesh is written by the same writer as `triangles`.
- **`table-link`** writes the link of a column to a table, or of a property to the
  table of its axes, in the convention the library reads first; see
  [Writing a link](tables.md#writing-a-link).
- **`along-well-view`** stores a drawn [geosteering section](along-well-section.md) in
  the package: the SVG (and a PNG of that same SVG, when the caller gives one) as
  attachments of the frame, and an `Activity` whose inputs are the objects the section was
  computed from. What the section was computed from is read from the drawing's own
  metadata, an object it names that the package does not hold is refused by its uri, and
  a setting given that disagrees with the drawing is refused. `stored_views` reads such
  results back by the template of their activity, never by their title.

The decisions are in [ADR 0037](adr/0037-results-are-artefacts-promoted-on-demand.md)
and [ADR 0039](adr/0039-several-values-per-element-written.md). Each of these operations
has its own canonical `ActivityTemplate`, like every other (see
[Provenance](provenance.md)).

## Using it from the command line

```sh
energyml create crs --set epsg=2154 --set vertical=5720 --plan     # the quote
energyml create crs --set epsg=2154 --set vertical=5720 --out crs.epc
energyml create polylines --from model.epc#<uuid> --set title="Copy" --into model.epc --out model+copy.epc
energyml create interpolate --from model.epc#<uuid> --set method=idw --set spacing=100 --into model.epc --out model+map.epc
```

`--set name=value` is generic; `--list` shows what each parameter is, which it
depends on (*with*), which it excludes (*not with*), and what leaving it out
makes (*[empty: creates ...]*).

## Using it from Rust

```rust
use energyml_build::{apply, operation::Registry};

let registry = Registry::new();                       // the built-in operations
let offers = registry.offers(&pieces);                // what fits, with refusals named
let plan = registry.plan("crs", &pieces, &choices, &identity, &when)?;
// ... inspect `plan`, then, and only then:
let made = apply::apply(&plan, &mut store, &when);
```

A caller can also start from `Registry::bare()` and `register` operations of its
own; a registered operation is tried ahead of the built-ins.

## Recipes

A recipe is a versioned TOML file of ordered steps, each an operation with its
settings and its inputs. `energyml create --recipe` plays every step through the
same door into one package and writes it at the end. Played, the recipe is also a
workflow: an `ActivityTemplate` with one sub-activity per step, and a root
`Activity` that every step's activity names as its parent. A step that took a file
records it as an `inputFile` parameter, so the workflow can be read back (and a
recipe drafted from it) without the original recipe; see [Provenance](provenance.md).

```sh
energyml create --recipe recipe.toml --plan               # what each step would make; writes nothing
energyml create --recipe recipe.toml --out Lutetia.epc    # the package, and Lutetia.h5 beside it
```

A sketch (TODO(verify): the key names below are those of the current parser and
are being translated to English; check them against the published version):

```toml
titre    = "Lutetia"                 # the package title
creation = "2026-09-24T00:00:00Z"    # stamped on every object: two runs, one package
auteur   = "Geosiris"                # every Citation's Originator
donnees  = "Lutetia.h5"              # the HDF5 file for steps that do not name another

[[etape]]
nom       = "repere"                 # unique: later steps refer to what this one wrote by it
operation = "crs"
set       = { epsg = 2154, vertical = 5720 }

[[etape]]
nom       = "sol"
operation = "interpolate"
entrees   = [ { creneau = "shape", etape = "repere", type = "PointSetRepresentation" } ]
set       = { spacing = 25.0, title = "Ground" }
```

An input refers to what an earlier step wrote **by that step's name** (narrowed
by `type`, `titre` or `role`), never by a uuid copied from a run. Everything is
checked before any step plays: unknown keys, unknown operations, duplicate step
names, settings the operation does not declare, inputs from a step that comes
later. A step marked as waiting is checked and reported, not played - never
commented out, which would leave nothing to report.

The reference model [`energyml-lutetia`](https://github.com/geosiris-technologies/energyml-lutetia)
is replayed from a recipe this way.

## Logging

Spans carry the elapsed time of each operation, recipe step, read and write; see
[Logging](logging.md).

## Writing your own

See the design notes in [ADR 0017](adr/0017-operation-is-a-pure-function.md). In
short: implement the operation trait with a `plan` that uses only its arguments,
declare `needs`, `takes`, `gives`, push it to a `Registry`, and the CLI, the
Python bridge and the Galaxy generator all find it.
