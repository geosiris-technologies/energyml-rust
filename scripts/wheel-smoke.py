#!/usr/bin/env python3
"""Smoke-test an installed `energyml-rust` wheel: import it, open a package and
read one array from its HDF5 file.

    python scripts/fetch-fixtures.py
    python scripts/wheel-smoke.py target/fixtures/fesapi/testingPackageCpp22.epc

Reading an array is the point: the wheels carry HDF5 compiled in, and a module
that imports but cannot read a `.h5` would pass any lesser test. Run it from
outside the source tree, so that the installed module is the one imported.
"""

import sys

from energyml.rust.epc import Epc


def main(path: str) -> int:
    pkg = Epc.open(path)
    objects = pkg.objects()
    if not objects:
        print(f"wheel-smoke: {path} opened with no object", file=sys.stderr)
        return 1
    for one in objects:
        for array in pkg.arrays(one.uri):
            values, shape = pkg.read_array(array)
            if len(values) == 0:
                print(f"wheel-smoke: {array} read empty", file=sys.stderr)
                return 1
            print(f"{len(objects)} objects; {one.qualified_type} {one.title!r}: "
                  f"{array.path_in_file} read, shape {shape}")
            return 0
    print(f"wheel-smoke: no array found in {path}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    sys.exit(main(sys.argv[1]))
