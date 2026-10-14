#!/usr/bin/env python3
"""Smoke-test an installed `energyml-rust` wheel: import it, open a package,
read one array from its HDF5 file, and check the shape of what `geometry_of`
hands back.

    python scripts/fetch-fixtures.py
    python scripts/wheel-smoke.py target/fixtures/fesapi/testingPackageCpp22.epc

Reading an array is the point: the wheels carry HDF5 compiled in, and a module
that imports but cannot read a `.h5` would pass any lesser test. Run it from
outside the source tree, so that the installed module is the one imported.

The geometry check holds the dictionaries the bridge builds, which no Rust test
sees: `index` is the patch's number on every patch, and the cell numbers of an
IJK grid are `cell_index`. They once shared `index`, the array replacing the
number on a grid. It needs a package with IJK grids, and refuses one without,
since a check that met no grid would prove nothing.
"""

import sys

import numpy as np

from energyml.rust.epc import Epc
from energyml.rust.resqml import geometry_of


def fail(why: str) -> int:
    print(f"wheel-smoke: {why}", file=sys.stderr)
    return 1


def main(path: str) -> int:
    pkg = Epc.open(path)
    objects = pkg.objects()
    if not objects:
        return fail(f"{path} opened with no object")

    read = next(((one, array) for one in objects for array in pkg.arrays(one.uri)), None)
    if read is None:
        return fail(f"no array found in {path}")
    one, array = read
    values, shape = pkg.read_array(array)
    if len(values) == 0:
        return fail(f"{array} read empty")
    print(f"{len(objects)} objects; {one.qualified_type} {one.title!r}: {array.path_in_file} read, shape {shape}")

    patches = grids = 0
    for one in pkg.find("Representation"):
        try:
            shape = geometry_of(pkg, one.uri)
        except Exception:
            continue  # a representation this build does not evaluate is refused by name
        for patch in shape["patches"]:
            patches += 1
            if not isinstance(patch["index"], int):
                return fail(f"{one.uri}: patch `index` is {type(patch['index']).__name__}, not the patch's number")
            if patch["topology"] == "hexahedra":
                grids += 1
                cell_index = patch.get("cell_index")
                if not isinstance(cell_index, np.ndarray):
                    return fail(f"{one.uri}: hexahedra without a `cell_index` array")
                if len(cell_index) not in (0, len(patch["cells"]) // 8):
                    return fail(f"{one.uri}: {len(cell_index)} cell numbers for {len(patch['cells']) // 8} cells")
    if grids == 0:
        return fail(f"no IJK grid evaluated in {path}: the geometry check would prove nothing")
    print(f"{patches} patches evaluated, {grids} of them hexahedra: `index` and `cell_index` as they should be")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    sys.exit(main(sys.argv[1]))
