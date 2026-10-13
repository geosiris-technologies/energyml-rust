"""Build a package from nothing: a CRS, a feature, an interpretation, a
representation and a property.

    python 02_a_package_from_nothing.py <out-directory>

Where the first example edited a package someone else wrote, this one starts
with an empty one and puts in the whole chain RESQML asks for:

    BoundaryFeature          what exists in the ground
      HorizonInterpretation  what someone decided it is
        PointSetRepresentation   where they put it — points, in a CRS
          ContinuousProperty     and a number on each point

None of these five objects is special to the bridge: they are dictionaries, and
`$type` is what says which class each one is.
"""

import os
import sys
import uuid

import numpy as np

from energyml.rust.epc import ArrayRef, Epc

# The projected CRS the points are given in — Lambert-93, the French national
# grid — and a vertical CRS for the elevations. **The geometry points at the
# projected CRS itself**, not at a compound one: EML 2.3 has no top-level
# compound of a projected and a vertical CRS, so the pair is written and the
# vertical one is referenced by nothing. That is a known gap of the schema, not
# an omission here.
EPSG = 2154

# A property kind from the Energistics dictionary. It is **not** in the package
# and does not have to be: a well-known kind resolves by its uuid, and writing a
# local copy of it would be one more object saying what the standard already
# says.
ELEVATION = {
    "$type": "eml23.DataObjectReference",
    "QualifiedType": "eml23.PropertyKind",
    "Title": "elevation",
    "Uuid": "8508cef4-0ade-454a-9ea4-e00d5905e677",
}

NOW = "2026-09-19T12:00:00Z"


def main(out_dir: str) -> None:
    os.makedirs(out_dir, exist_ok=True)
    epc = os.path.join(out_dir, "top-chalk.epc")
    data_file = "top-chalk.h5"

    pkg = Epc.create("top-chalk.epc")
    # An empty package has no path, so nothing says where its `.h5` should go.
    # Opening a package answers that question — beside the `.epc` — and
    # creating one cannot, so it is asked here.
    pkg.set_data_root(out_dir)

    # 1. The CRS pair. Both are EML 2.3 objects in a RESQML 2.2 package, which
    #    is normal: a release of RESQML names the release of EML it stands on.
    crs = {
        "$type": "eml23.ProjectedCrs",
        "uuid": str(uuid.uuid4()),
        "schemaVersion": "2.3",
        "Citation": citation(f"EPSG:{EPSG}"),
        "AbstractProjectedCrs": {"$type": "eml23.ProjectedEpsgCrs", "EpsgCode": EPSG},
        "AxisOrder": "easting northing",
        "uom": "m",
    }
    vertical = {
        "$type": "eml23.VerticalCrs",
        "uuid": str(uuid.uuid4()),
        "schemaVersion": "2.3",
        "Citation": citation("elevation above sea level"),
        "AbstractVerticalCrs": {
            "$type": "eml23.VerticalUnknownCrs",
            "Unknown": "no vertical datum was given",
        },
        "Direction": "up",
        "uom": "m",
    }

    # 2. The feature: what is in the ground, independently of anyone's opinion.
    feature = {
        "$type": "resqml22.BoundaryFeature",
        "uuid": str(uuid.uuid4()),
        "schemaVersion": "2.2",
        "Citation": citation("Top Chalk"),
    }

    # 3. The interpretation: someone's decision about that feature, in a domain.
    #    `depth` rather than `time` — these points are metres, not seconds, and
    #    the two are not interchangeable.
    interpretation = {
        "$type": "resqml22.HorizonInterpretation",
        "uuid": str(uuid.uuid4()),
        "schemaVersion": "2.2",
        "Citation": citation("Top Chalk, picked on seismic"),
        "Domain": "depth",
        "InterpretedFeature": reference(feature),
    }

    # 4. The representation: where the interpretation has been put. A scatter
    #    of picks rather than a surface — four points on a gentle dome.
    points = np.array(
        [
            [650000.0, 6860000.0, -1250.0],
            [650500.0, 6860000.0, -1180.0],
            [650500.0, 6860500.0, -1205.0],
            [650000.0, 6860500.0, -1262.0],
        ],
        dtype=np.float64,
    )
    picks_uuid = str(uuid.uuid4())
    points_path = f"/resqml22/{picks_uuid}/points_patch0"
    picks = {
        "$type": "resqml22.PointSetRepresentation",
        "uuid": picks_uuid,
        "schemaVersion": "2.2",
        "Citation": citation("Top Chalk picks"),
        "RepresentedObject": reference(interpretation),
        "NodePatchGeometry": [
            {
                "$type": "resqml22.PointGeometry",
                "LocalCrs": reference(crs),
                "Points": {
                    "$type": "resqml22.Point3dExternalArray",
                    "Coordinates": external_array(data_file, points_path, points.size),
                },
            }
        ],
    }

    # 5. And a number on each point. `float32` on purpose: the values cross in
    #    the type the array carries, and a property does not have to be as
    #    precise as the geometry it hangs on.
    elevations = np.array([-1250.0, -1180.0, -1205.0, -1262.0], dtype=np.float32)
    property_uuid = str(uuid.uuid4())
    values_path = f"/resqml22/{property_uuid}/values_patch0"
    elevation = {
        "$type": "resqml22.ContinuousProperty",
        "uuid": property_uuid,
        "schemaVersion": "2.2",
        "Citation": citation("Top Chalk elevation"),
        "IndexableElement": "nodes",
        "SupportingRepresentation": reference(picks),
        "PropertyKind": ELEVATION,
        "Uom": "m",
        "ValueCountPerIndexableElement": [1],
        "ValuesForPatch": [
            {
                "$type": "eml23.FloatingPointExternalArray",
                "ArrayFloatingPointType": "arrayOfFloat32LE",
                "CountPerValue": 1,
                "Values": external_array(data_file, values_path, elevations.size),
            }
        ],
    }

    for one in (crs, vertical, feature, interpretation, picks, elevation):
        print(f"put {pkg.put(one)}")

    pkg.write_array(ArrayRef(data_file, points_path), points.ravel(), [len(points), 3])
    pkg.write_array(ArrayRef(data_file, values_path), elevations, [len(elevations)])

    said = pkg.save_as(epc)
    print(f"saved {epc} with {len(said)} diagnostic(s)")
    for one in said:
        print(f"  ! {one}")

    check(epc, picks_uuid, property_uuid)


def citation(title: str) -> dict:
    """The one part of an object that a human will read."""
    return {
        "$type": "eml23.Citation",
        "Title": title,
        "Originator": "energyml-python example",
        "Creation": NOW,
        "Format": "energyml-rust",
    }


def reference(obj: dict) -> dict:
    """A DataObjectReference to an object being built here.

    It is made from the dictionary rather than from a summary, because these
    objects are not in the package yet — the reference has to exist before the
    thing it names is put.
    """
    return {
        "$type": "eml23.DataObjectReference",
        "QualifiedType": obj["$type"],
        "Title": obj["Citation"]["Title"],
        "Uuid": obj["uuid"],
    }


def external_array(file: str, path: str, count: int) -> dict:
    """Where an array's numbers are: a file, a path in it, and how many."""
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


def check(epc: str, picks_uuid: str, property_uuid: str) -> None:
    """Read the package back — the only statement worth making about a write."""
    again = Epc.open(epc)
    print(f"{len(again.objects())} objects read back:")
    for one in again.objects():
        print(f"  {one.qualified_type:38} {one.title}")

    values, dimensions = again.read_array(again.arrays(again.uri_of(picks_uuid))[0])
    print(f"points     {values.dtype} {dimensions}\n{values.reshape(dimensions)}")
    values, dimensions = again.read_array(again.arrays(again.uri_of(property_uuid))[0])
    print(f"elevations {values.dtype} {dimensions} {values}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    main(sys.argv[1])
