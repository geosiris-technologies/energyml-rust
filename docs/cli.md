# The command line

`energyml` is the library with argument parsing in front of it. Nothing the
command does is out of reach of an application: every check it prints is a call
you can make yourself. (`enyml` is installed as a short alias of the same
binary - TODO(verify).)

Every option of every command is in the generated
[`cli-reference.md`](cli-reference.md). This page is the how and the why.

Some commands exist only in some builds. Commands that read package arrays need
the `hdf5` feature; `profile --png` needs `png`. ETP commands (talking to a
remote dataspace) belong to the separate
[`retp`](https://github.com/geosiris-technologies/retp) project. `galaxy-tool`
and `cli-reference` are hidden from `--help` (see [Galaxy tools](galaxy.md)).

Global flags worth knowing: `--lenient` (read past a missing required element or
an unreadable value, reporting each), `--strict` (refuse; use it when the objects
will be written back), `--h5 [NAME=]PATH` (offer an HDF5 file whatever the
package says), `--log` (see [Logging](logging.md)).

## Looking at a package

```sh
energyml list model.epc                      # uuid, type, title; --filter grid, --quick
energyml check model.epc                     # three independent checks; exit 1 on errors
energyml check model.epc --errors-only --no-schema
energyml show model.epc <uuid>               # one object; --json, --with-data, --raw
energyml arrays model.epc --summary          # where the numbers live; --values N needs hdf5
energyml properties model.epc --check        # what the numbers mean, and counts that disagree
energyml colors model.epc                    # what colour each object has, and which level said so
energyml tables model.epc                    # lookup tables behind discrete properties
energyml wells model.epc --out wells.csv     # wellbores as a table, one row per node
energyml series model.epc <uuid>             # a log, a time series, a flow test as series
energyml geometry model.epc                  # what can be drawn, how many points, which CRS
energyml graph model.epc --format dot -o graph.dot    # the reference graph
energyml types --describe TrianglePatch      # a schema type, field by field
energyml docs --out docs/                    # the model's own documentation as JSON
```

`geometry` ends with what it could **not** evaluate, by name: a viewer that
shows nothing and a viewer with nothing to show look identical, so the forms a
build does not evaluate are said aloud rather than counted as zero points.

`graph` writes one node per object (uri, uuid, qualified type, the
version-independent *concept*, title) and one edge per reference. An object the
package references without holding gets `present: false`; dropping those would
draw a self-contained package where there is none.

## Exporting

```sh
energyml export model.epc --list                      # the formats this build has
energyml export model.epc --format obj -o out/        # one file per representation
energyml export model.epc --format geojson --metadata metadata.json -o out/
energyml export model.epc --format vtu --epsg 3857 --properties -o out/
```

A directory export names files `qualifiedType_uuid.ext`: the uuid makes it
unique (titles collide far more often than expected) and the qualified type
makes the folder readable. **One representation that cannot be written does not
stop the others**: it is named with the reason and the batch carries on.

`--as PATTERN=SHAPE` chooses the form each type comes out in (`points`, `lines`,
`surface`, `volume`, `auto`, plus the summaries `outline` and `centroid`). The
pattern matches anywhere in the qualified type, case-insensitively; rules are
tried in order, first match wins. **A shape an object cannot take is an error,
not a fallback**:

| From | points | lines | surface | volume |
|---|:-:|:-:|:-:|:-:|
| point set | yes | no | no | no |
| polylines | yes | yes | no | no |
| triangulated set | yes | no | yes | no |
| 2D grid | yes | no | yes | no |
| IJK grid | yes | no | yes (its outside) | yes |
| unstructured grid | yes | no | no | yes |

An IJK grid asked for as a surface gives its outside - the faces used by exactly
one cell - not every face twice. GeoJSON is always WGS 84, and every feature
carries a `bbox`.

## Repairing, relabelling, renaming

```sh
energyml repair model.epc                          # the parts no parser will touch, and why
energyml repair model.epc --extract broken/        # their bytes, under their part names
energyml repair model.epc --apply broken/ --out fixed.epc
energyml relabel model.epc --package resqml --schema-version 2.2_dev3 -o fixed.epc
energyml h5 model.epc                              # where each array really is
energyml h5 model.epc --fix fixed.epc
energyml rename model.epc --to survey.h5           # the quote: fields, objects, relationships
energyml rename model.epc --to survey.h5 --apply   # and the file itself
energyml extract model.epc --select choice.json --out part.epc --dry-run
```

`repair` is for the parts every other command must refuse (markup that does not
close, bytes that are not UTF-8, an empty part): bytes out, fix, bytes back, and
`--apply` says for each part whether it now parses. `--lenient` makes the read
levels a ladder: what `repair` lists is what **that mode** cannot read.
`rename` and `extract` always quote first (`--dry-run` / no `--apply`).

## Selecting by region

```sh
energyml select model.epc --type WellboreTrajectory --outer zone.geojson --rings-epsg 2154 --test head
```

Prints the uuids (or `--format uri|json`) of the objects that fall in a region
drawn by outer rings, minus inner ones (`--inner`), or its complement (`--invert`).

## Creating objects

`energyml create` is the command form of the operations engine; see
[Creating objects](creating-objects.md).

```sh
energyml create --list                                   # what can be made from nothing
energyml create --list --from export.geojson             # from this file, refusals named
energyml create crs --set epsg=2154 --set vertical=5720 --plan   # the quote
energyml create crs --set epsg=2154 --out crs.epc                # the write
energyml create --recipe recipe.toml --plan
energyml create --recipe recipe.toml --out Lutetia.epc
```

Parameters are the operation's to declare, so there is one generic
`--set name=value` rather than an option per parameter. `--list` prints each with
its kind, whether it is required, what it depends on or excludes, and what
leaving it out makes. A value is read as JSON when it is one (`2154`, `true`) and
as text otherwise. `--now` fixes the instant, so two equal runs write the same
package byte for byte.

`energyml import file.json --plan` reads a GeoJSON or a GeoSciML `GeologicUnit`
export and writes the objects it describes (quote first).

## Public data

```sh
energyml sources                                  # the APIs this build can query
energyml sources brgm-bss                         # collections, filters, suggested values
energyml fetch brgm-bss boreholes --area area.geojson --where codeSpace=BRGM     # the quote
energyml fetch brgm-bss boreholes --area area.geojson --where codeSpace=BRGM --out bss.json
energyml terrain --list --area 651000,6860000,653000,6862000
energyml terrain --area 651000,6860000,653000,6862000 --step 25 --set vertical=5720 --out dem.epc
```

`fetch` and `terrain` quote first and download only when told to. Nothing about a
provider is spelled in the CLI: it is declared by the provider and read back as
data. See [Public data sources](data-sources.md).

## Section view

```sh
energyml profile model.epc --track track.geojson --left 300 --right 300 --svg long.svg
energyml profile model.epc --track track.geojson --left 300 --right 300 --draw plan --svg plan.svg
energyml profile model.epc --track track.geojson --left 300 --right 300 --cross-every 300 --draw cross:2 --svg cross-2.svg
```

See [Section view](section-view.md).

## House rules

```sh
energyml rules --house                         # what this library decides where the spec is silent
energyml rules --house --theme colour --json
```

See [House rules](house-rules.md).

## Exit codes

`check` exits 1 when it finds errors. A refusal that is a missing capability (a
build without `hdf5`) says which feature is missing; it never prints an empty
result in its place.
