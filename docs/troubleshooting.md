# Troubleshooting

Problems a user can hit, by symptom.

## Building with HDF5

Reading and writing arrays needs the `hdf5` feature, and therefore the HDF5
library.

### Windows: `Unable to infer HDF5 library runtime version`

The build script of `hdf5-metno-sys` fails with this message, which looks like a
`PATH` or `HDF5_DIR` problem.

- **Cause**: the installed `hdf5.dll` is the Intel/Fortran variant, which imports
  `libmmd.dll` (an oneAPI runtime) that is not on the disk. The message never
  names the real cause. `dumpbin /DEPENDENTS hdf5.dll` shows `libmmd.dll` when it
  is the case.
- **Solution**: use only the official **MSVC build of HDF5 1.14.6**
  (`hdf5-1.14.6-win-vs2022_cl.zip`), installed outside `Program Files`, with
  `HDF5_DIR` pointing at it. Copy its `hdf5.dll` next to the executable (the
  executable's folder wins over `PATH`).

### `Invalid H5_VERSION: 2.2.0`

The version check of `hdf5-metno-sys` only accepts a fixed set of minor versions
and does not know HDF5 2.x. Stay on HDF5 1.14.6. If a 2.x install is earlier on
`PATH`, test binaries load it: this works in practice but is not the version they
were compiled against.

### `h5dump` / `h5ls` print nothing and exit 53

Some installed builds are unusable. Do not spend time on them: to check what an
HDF5 file really holds from outside the library, read it with `h5py`.

### Binaries without HDF5

Where HDF5 cannot be compiled, a binary can be built without it. It then
**refuses by name** to read an array (`FeatureNotEnabled`) instead of returning an
empty one. An empty result with no error is never the library's way of saying
"no HDF5".

## Python

### `ImportError: DLL load failed` on Windows

`import energyml.rust` fails on a machine without HDF5, and putting HDF5 on
`PATH` does not help (Python 3.8+ no longer searches `PATH` for the dependencies
of an extension module).

- **Cause**: a wheel built locally without bundling HDF5. `maturin --auditwheel`
  is a Linux flag; on Windows it does nothing and does not say so.
- **Solution**: repair the wheel with `delvewheel`, naming the namespace package
  so that it does not write `energyml/__init__.py` over the other `energyml.*`
  distributions:

  ```sh
  delvewheel repair --namespace-pkg energyml --add-path "$HDF5_DIR/bin" -w wheels staging/*.whl
  ```

  Released wheels already carry HDF5.

### Another `energyml.*` package disappeared after install

Two distributions that both ship `energyml/__init__.py` overwrite each other. This
bridge ships none. If one is missing, reinstall the other.

## Opening and checking packages

### Arrays cannot be found after a rename or move

An EPC names its `.h5` in every array pointer and in the `.rels` of parts that go
through a proxy. If you renamed or moved the file by hand, the package opens and
its arrays are unreachable.

- Find where each array really is: `energyml h5 model.epc`; write the answer into
  the package with `energyml h5 model.epc --fix fixed.epc`.
- Offer the file explicitly: `energyml --h5 renamed.h5 ...`, or
  `--h5 model.h5=path/to/renamed.h5` to pin a path to the name the document uses.
- To rename on purpose, use `energyml rename model.epc --to new.h5` (quote), then
  `--apply`: fields, relationships and the file change together.

### `rename` fails on Windows with a file in use

An HDF5 handle still open on a file locks it. The rename operation releases the
handles it holds first; close any other program that has the file open.

### An object is read in the wrong release, or as a development drop

Check the declared versions: `energyml list model.epc`. A package written against
a development drop but declaring the release reads as the release and silently
drops what only the drop has. `energyml relabel model.epc --package resqml
--schema-version 2.2_dev3` corrects the declaration without reading the content.

### A part is refused

`energyml repair model.epc` lists the parts no parser will touch (markup that does
not close, invalid UTF-8, an empty part) with the reason. `--extract` writes their
bytes out, `--apply` puts repaired bytes back. `--lenient` reads past an absent
required element or an unreadable value and reports each; do not save what it
read, because it holds values nobody wrote (use `--strict` when writing).

### `check` reports a dangling reference to a PropertyKind

A well-known `PropertyKind` of the Energistics catalogue is absent from a package
by design; such references are reported as catalogue references and count as
neither error nor warning. Any other dangling reference is real.

### A coordinate looks plausible and is wrong

Check the unit and the placement: `energyml geometry model.epc --points
--metres`. A file whose horizontal unit is feet read as metres is 3.28 times too
far from the origin with every number looking reasonable. A unit the library does
not know is refused, not assumed.

### GeoJSON lands in the wrong place

GeoJSON is always WGS 84. If your export lands in the Gulf of Guinea, the source
CRS carries no usable EPSG code; check `energyml geometry` for the CRS the
package states. TODO(verify): behaviour and wording when a package states no EPSG
code.

## Exports

### "evaluated but could not be written"

One representation that cannot be written does not stop the others. The line
names the object and the reason, for example a travel-time CRS that needs a
velocity model the package does not carry. This is the usual shape of a refusal:
the geometry evaluated fine and the placement has no answer.

## Docker

Files are written by uid 1000 inside the container. On a Linux host, set the
ownership of the mounted folder accordingly; Docker Desktop maps it to your user.
An EPC and its `.h5` must both be inside the mounted folder.

## Still stuck

Run the failing command with `--log debug` ([Logging](logging.md)) and open an
issue with the output and the exact version (`energyml --version`).
