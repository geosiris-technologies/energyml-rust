"""Add a triangulated surface to an existing package.

Opens an EPC, looks through what is already in it — a CRS to place the points
in, an interpretation for the surface to represent — writes a new
`TriangulatedSetRepresentation` with its own citation, puts its points and
triangles in the package's HDF5 file, and saves the whole thing somewhere else.

    python 01_add_a_surface.py <in.epc> <out-directory>

Nothing here knows a RESQML class: an object is a dictionary, and the `$type`
is what says which class it is.
"""

import json
import os
import shutil
import sys
import uuid

import numpy as np

from energyml.rust.epc import ArrayRef, Epc, Summary


def main(source: str, out_dir: str) -> None:
    # The package **and its data file**: the arrays are written into the `.h5`
    # that sits beside the `.epc`, so both are copied before anything is
    # touched. Editing the original in place would be the one thing a caller
    # cannot undo.
    os.makedirs(out_dir, exist_ok=True)
    stem = os.path.splitext(os.path.basename(source))[0]
    epc_path = os.path.join(out_dir, os.path.basename(source))
    shutil.copy(source, epc_path)
    data_file = f"{stem}.h5"
    shutil.copy(os.path.join(os.path.dirname(source), data_file), os.path.join(out_dir, data_file))

    pkg = Epc.open(epc_path)
    print(f"{len(pkg.objects())} objects in {os.path.basename(epc_path)}")

    # 1. What the package already holds. A summary costs no parsing, so this
    #    scan is cheap even on a package of several gigabytes.
    crs = depth_crs(pkg)
    interpretation = one_of(pkg, "GenericFeatureInterpretation")
    print(f"  crs             {crs.title}  ({crs.uuid})")
    print(f"  interpretation  {interpretation.title}  ({interpretation.uuid})")

    # 2. The shape. A flat array of coordinates — RESQML writes its points flat
    #    — and the triangles as indices into it. This one is a square split in
    #    two, lifted on one corner so it is not flat.
    points = np.array(
        [
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [100.0, 100.0, 25.0],
            [0.0, 100.0, 0.0],
        ],
        dtype=np.float64,
    )
    triangles = np.array([[0, 1, 2], [0, 2, 3]], dtype=np.int32)

    # 3. The object. Its uuid is ours, and the two arrays are named here
    #    **before** they are written: the object says where its numbers are,
    #    and writing them is a separate step that can fail on its own.
    surface_uuid = str(uuid.uuid4())
    points_path = f"/resqml22/{surface_uuid}/points_patch0"
    triangles_path = f"/resqml22/{surface_uuid}/triangles_patch0"

    surface = {
        "$type": "resqml22.TriangulatedSetRepresentation",
        "uuid": surface_uuid,
        "schemaVersion": "2.2",
        "Citation": {
            "$type": "eml23.Citation",
            "Title": "A ramp, added from Python",
            "Originator": "energyml-python example",
            "Creation": "2026-09-19T10:00:00Z",
            "Format": "energyml-rust",
            "Description": "Two triangles, written by 01_add_a_surface.py",
        },
        "RepresentedObject": reference(interpretation),
        "SurfaceRole": "map",
        "TrianglePatch": [
            {
                "$type": "resqml22.TrianglePatch",
                "NodeCount": len(points),
                "Geometry": {
                    "$type": "resqml22.PointGeometry",
                    "LocalCrs": reference(crs),
                    "Points": {
                        "$type": "resqml22.Point3dExternalArray",
                        "Coordinates": external_array(data_file, points_path, points.size),
                    },
                },
                "Triangles": {
                    "$type": "eml23.IntegerExternalArray",
                    "ArrayIntegerType": "arrayOfInt32LE",
                    "CountPerValue": 1,
                    "NullValue": -1,
                    "Values": external_array(data_file, triangles_path, triangles.size),
                },
            }
        ],
    }

    written = pkg.put(surface)
    print(f"put {written}")

    # 4. The numbers. Flat, with the shape beside them — the same convention as
    #    when they are read back.
    pkg.write_array(ArrayRef(data_file, points_path), points.ravel(), [len(points), 3])
    pkg.write_array(
        ArrayRef(data_file, triangles_path), triangles.ravel(), [len(triangles), 3]
    )

    # 5. And out. The diagnostics are **returned** rather than printed by the
    #    library, so a caller that ignores them does so knowingly. Most of
    #    these belong to the package as it was found — a reference to an object
    #    that is not in it cannot be given a relationship — and saying how many
    #    there are is how a new one becomes visible.
    said = pkg.save_as(epc_path)
    print(f"saved with {len(said)} diagnostic(s)")
    for one in said[:3]:
        print(f"  ! {one}")

    check(epc_path, surface_uuid)


def depth_crs(pkg: Epc) -> Summary:
    """A CRS whose vertical axis measures depth rather than time.

    A package can hold both, and they are told apart by what they *say* —
    `VerticalAxis.IsTime` — not by their titles. Placing points measured in
    metres in a CRS whose vertical unit is a second would produce a package
    that opens perfectly and means something else.
    """
    for one in pkg.objects():
        if not one.qualified_type.endswith("LocalEngineeringCompoundCrs"):
            continue
        axis = pkg.get(one.uri).get("VerticalAxis", {})
        if axis.get("IsTime") is False:
            return one
    raise SystemExit("this package holds no depth CRS")


def one_of(pkg: Epc, type_name: str) -> Summary:
    """The first object of a type, or a refusal that says what was looked for."""
    for one in pkg.objects():
        if one.qualified_type.endswith(type_name):
            return one
    raise SystemExit(f"this package holds no {type_name}")


def reference(summary: Summary) -> dict:
    """A DataObjectReference to an object of this package."""
    return {
        "$type": "eml23.DataObjectReference",
        "QualifiedType": summary.qualified_type,
        "Title": summary.title,
        "Uuid": summary.uuid,
    }


def external_array(file: str, path: str, count: int) -> dict:
    """Where an array's numbers are, in the package's own vocabulary.

    `Count` is the number of **values**, not of points: a surface of four
    points has twelve coordinates, and the array is flat.
    """
    return {
        "$type": "eml23.ExternalDataArray",
        "ExternalDataArrayPart": [
            {
                "$type": "eml23.ExternalDataArrayPart",
                "Count": [count],
                "StartIndex": [0],
                "MimeType": "application/x-hdf5",
                "PathInExternalFile": path,
                "URI": file,
            }
        ],
    }


def check(epc_path: str, surface_uuid: str) -> None:
    """Read the package back and confirm the surface is in it, numbers and all.

    A write that reported nothing and produced a package missing an object
    would look exactly like a write that worked.
    """
    again = Epc.open(epc_path)
    uri = again.uri_of(surface_uuid)
    if uri is None:
        raise SystemExit("the surface is not in the package that was written")
    obj = again.get(uri)
    print(f"read back: {obj['Citation']['Title']} ({obj['$type']})")
    for ref in again.arrays(uri):
        values, dimensions = again.read_array(ref)
        print(f"  {ref.path_in_file}  {values.dtype}  {dimensions}")
        print(f"    first row: {values.reshape(dimensions)[0]}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    main(sys.argv[1], sys.argv[2])
