# Getting started

Ten minutes, three ways in — Rust, Python, command line — all on the same file:
`package/Lutetia.epc` (with its `Lutetia.h5`) from the reference model
[`energyml-lutetia`](https://github.com/geosiris-technologies/energyml-lutetia).
Lutetia is a synthetic, documented, multi-domain model written in RESQML 2.2,
EML 2.3, WITSML 2.1 and PRODML 2.x. Download the two files side by side in one
folder: an EPC names its data file relatively, so they must stay together.

```sh
git clone https://github.com/geosiris-technologies/energyml-lutetia
ls energyml-lutetia/package      # Lutetia.epc  Lutetia.h5
```

> An EPC is a zip of XML parts (the objects) plus relationship files; the big
> numeric arrays live next to it in an HDF5 file. Reading the arrays needs the
> `hdf5` feature (see [Troubleshooting](troubleshooting.md) if it does not build).

## Install

| You want | Do |
|---|---|
| the command line | `cargo install energyml-cli --features hdf5` (installs `energyml`; `enyml` is an alias) — TODO(verify): crates.io name and release binaries |
| the Rust libraries | `cargo add energyml --features all` (or only the versions you read, for example `--features resqml22`; the `energyml` crate turns no schema version on by itself) — TODO(verify): crates.io name |
| Python | `pip install energyml-rust` (imported as `energyml.rust`) — TODO(verify): final distribution name on PyPI |
| a container | `docker pull ghcr.io/geosiris-technologies/energyml` — TODO(verify): image not yet published, see [Docker](docker.md) |

## Command line

```sh
energyml list package/Lutetia.epc                 # one line per object: uuid, type, title
energyml check package/Lutetia.epc                # three independent checks, exit code 1 on errors
energyml geometry package/Lutetia.epc             # what can be drawn, how many points, which CRS
energyml export package/Lutetia.epc --format geojson -o out/    # WGS 84 GeoJSON
energyml export package/Lutetia.epc --format vtu --properties -o out/
```

A section view of the model, as SVG (see [Section view](section-view.md)):

```sh
energyml profile package/Lutetia.epc \
  --track "650700,6861600;651300,6861750;652100,6862100" \
  --left 300 --right 300 --cross-every 300 --svg long.svg
```

Every option of every command is in the generated
[`cli-reference.md`](cli-reference.md). The overview is [The command line](cli.md).

## Rust

```toml
[dependencies]
# Lutetia is RESQML 2.2, EML 2.3, WITSML 2.1 and PRODML 2.x: name the versions you read, or "all".
energyml = { version = "0.1", features = ["all"] }
```

```rust
use std::sync::Arc;
use energyml::storage::EpcStore;

// Which schema versions this program understands is the application's choice:
// `energyml::schemas::all()` registers the versions whose feature is on.
let registry = Arc::new(energyml::schemas::all());

let store = EpcStore::open("package/Lutetia.epc", registry)?;

for summary in store.resources() {                // no object XML is opened
    let header = store.header(&summary.uri)?;     // stops at the end of the citation
    println!("{} - {}", summary.qualified_type, header.display_name());
}

for diagnostic in store.diagnostics() {           // what the package gets wrong
    eprintln!("{diagnostic}");
}
```

Reading a representation as something you can draw:

```rust
use energyml::geometry::{crs, geometry};

let object = store.get_object(&uri)?;
let shape = geometry::of(object.as_reflect(), &store)?;   // patches of points and topology
let (vertices, indices) = shape.merged();
let placed = crs::of(object.as_reflect(), &store)?;       // its coordinate reference system
```

The long form, with arrays, editing and exporting, is [The library](library.md).

## Python

```python
from energyml.rust.epc import Epc

pkg = Epc.open("package/Lutetia.epc")
for one in pkg.objects():
    print(one.qualified_type, one.uuid, one.title)

from energyml.rust.resqml import geometry_of, mesh_of
surface = pkg.find("TriangulatedSetRepresentation")[0]
points, triangles = mesh_of(pkg, surface.uri, "metres")   # flat numpy arrays
```

No `h5py` is needed: the arrays come out of the same HDF5 backend the command
line uses. See [Python](python.md).

## What next

- Make your own objects: [Creating objects](creating-objects.md). The Lutetia
  package itself is replayed from a recipe by `energyml create --recipe`.
- Check what the library decided where the specification is silent:
  [House rules](house-rules.md).
- Something does not build or does not open: [Troubleshooting](troubleshooting.md).
