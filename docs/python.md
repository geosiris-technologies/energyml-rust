# Python

A bridge over the Rust library: open, read, edit and write EPC packages and their
HDF5 files, ask for geometry and properties, export, check. Like the command line
it knows nothing the library does not; every answer is a call an application
would make itself in Rust.

The wheel is built with every schema version (`all`): nothing to choose from
Python. The per-version Cargo features of the `energyml` crate concern Rust
applications only (see [The library](library.md)).

**You do not need `h5py`**: arrays come out of the same HDF5 backend the command
line uses. Binary wheels carry the HDF5 library, so the machine that installs one
needs neither HDF5 nor a Rust toolchain.

## Name and install

The distribution is `energyml-rust` and the import is `energyml.rust` - a room in
the `energyml.*` namespace shared with the pure-Python generated model packages
(`energyml-resqml2-2` and siblings, `energyml-utils`), not a competitor to them:

```python
from energyml.resqml.v2_2 import resqmlv2   # generated classes (other distribution)
from energyml.rust.epc import Epc           # this bridge
```

The wheel ships no `energyml/__init__.py`, so it never overwrites another
distribution's files. TODO(verify): the final PyPI name and import path may
become plain `energyml`; confirm before publishing and update this page.

```sh
pip install energyml-rust        # TODO(verify): published on PyPI
```

Until then, build a wheel from the repository (needs Rust and HDF5):

```sh
pip install maturin
maturin build -m python/Cargo.toml --release     # TODO(verify): path of the bridge crate in the public repository
```

The wheel is `abi3`: one wheel per platform, not one per Python version. On
Windows, a locally built wheel must be repaired so it carries HDF5 (see
[Troubleshooting](troubleshooting.md), *DLL load failed*).

Out of scope on purpose: ETP. For a remote dataspace use the [`retp`](https://github.com/geosiris-technologies/retp) project.

## The map

| module | what is in it |
|---|---|
| `energyml.rust.storage` | the abstraction above every store; `Epc` is its first subclass |
| `energyml.rust.epc` | open, read, edit and write an EPC and its HDF5 files; ask its catalogue questions |
| `energyml.rust.model` | the generated model by reflection: which types exist, what they hold, an empty object of one |
| `energyml.rust.resqml` | geometry of a representation, values on it, export |

## Opening a package

```python
from energyml.rust.epc import Epc

pkg = Epc.open("package/Lutetia.epc")
for one in pkg.objects():
    print(one.qualified_type, one.uuid, one.title)
```

The `.h5` files are looked for beside the `.epc`. A summary is what is known
without opening the part; a part that cannot be parsed keeps its row with an
empty title (dropping it would say it is not in the package). Create an empty
package with `Epc.create("name.epc")` and tell it where data files go with
`set_data_root(directory)`.

```python
pkg.find("Representation")                  # by type, case-insensitive
pkg.find("Crs", "default")                  # and by title (both mean "and")
```

### What points at what

```python
for link in pkg.references(uri):
    print(link["qualified_type"], link["title"], link["path"], link["present"])

pkg.referenced_by(uri)        # the other direction (reads every object: no package indexes it)
needed, missing = pkg.needs([uri])   # the closure: what an extraction or a copy must carry
```

`present` says whether the package holds the target; a dangling reference keeps
its entry. A well-known `PropertyKind` from the Energistics dictionary is never
reported as dangling.

### Taking objects into another package

```python
small = Epc.create("small.epc")
small.set_data_root("out")
done = small.copy_from(pkg, [surface.uri])
done["added"]     # what came along: interpretation, feature, CRS...
done["arrays"]    # datasets that travelled
small.save_as("out/small.epc")
```

The set is closed on the way in: asking for a surface alone would give a package
that opens and places it nowhere.

## The model and making an object

```python
from energyml.rust.model import describe, template, types

types("resqml")                       # every RESQML type this build carries
describe("resqml22.BoundaryFeature")  # fields, cardinality, documentation
```

There are no Python classes, by decision: one class per type per release would be
a second copy of the generated model. You get the reflection and a skeleton:

```python
import uuid
obj = template("resqml22.BoundaryFeature", str(uuid.uuid4()))
obj["Citation"].update(Title="Top Chalk", Originator="me",
                       Creation="2026-09-19T12:00:00Z", Format="energyml-rust")
pkg.put(obj)
```

The uuid is always given, never invented: nothing defaults to a random one,
because a caller that derives identities from its source must get the same
objects on a second run.

## Objects travel as JSON

```python
obj = pkg.get(uri)                 # a dict
obj["Citation"]["Title"] = "Renamed"
pkg.put(obj)                       # back in; returns the URI it wrote
```

The `$type` key makes the JSON re-readable. A dict is checked against the schema
and a refusal names the field and value. `pkg.get(uri)` on an existing object of
the type you want to write is the best way to learn a shape.

## Arrays

```python
for ref in pkg.arrays(uri):
    print(ref.file, ref.path_in_file, pkg.array_shape(ref))

values, dimensions = pkg.read_array(ref)          # flat numpy array and the shape beside it
points = values.reshape(dimensions)
values, dimensions = pkg.read_array_part(ref, start=[1, 0], count=[1, 3])   # a slice

pkg.write_array(ref, np.asarray(points, dtype=np.float64).ravel(), [len(points), 3])
```

Arrays are **flat copies** (a view would outlive the memory it points to). The
dtype decides what is written (float32/64, int8-64, uint8-64); anything else is
refused by name. A shape that disagrees with the number of values is refused,
naming both counts.

## Geometry, properties and export

```python
from energyml.rust.resqml import geometry_of, mesh_of, properties_of, export_of, formats

shape = geometry_of(pkg, uri)                 # placed points and topology, by patch
points, triangles = mesh_of(pkg, uri, "metres")
values, problems = properties_of(pkg, uri)    # values hanging on this shape, with colour ramps
open("model.obj", "wb").write(export_of(pkg, [uri], "obj"))
```

`geometry_of` returns what the file says, patch by patch, with a `topology` key
(`points`, `triangles`, `quads`, `polygons`, `polylines`, `segments`,
`hexahedra`, `polyhedra`, `grid`). `placement` is `local`, `projected` (the
default) or `metres`; an unknown name is refused with the list, and `metres` is
refused when the unit cannot be converted. `mesh_of` merges and triangulates, and
says so in its name. `properties_of` returns the values that fit the shape,
checked against the geometry, and a second list of those that did not fit. A
colour ramp is empty when the package states none: no default is invented.

`export_of` returns bytes. The list of formats is the exporter registry's own. A
format that cannot write an object **refuses, naming it**.

## Judging and saving a package

```python
r = pkg.check()
print(r["errors"], r["warnings"], r["sound"])     # the same numbers `energyml check` prints

diagnostics = pkg.save_as("copy.epc")             # returned, never swallowed
```

`check(schema=False)` returns `checked: False`: not checked is not the same as
clean.

## Renaming the data file

```python
quote = pkg.plan_rename("survey.h5", "survey_2026.h5")   # changes nothing
done = pkg.rename_data_file("survey.h5", "survey_2026.h5")
```

Quote first; the operation refuses as a whole rather than doing part.

## Examples

Worked examples live in the bridge's `examples/` folder and are checked by its
tests: adding a surface to an existing package, and building a package from
nothing (feature, interpretation, point set, property). TODO(verify): path of
`examples/` in the public repository.
