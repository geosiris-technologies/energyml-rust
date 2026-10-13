"""What the library knows about RESQML: shapes, and the values on them.

    from energyml.rust.epc import Epc
    from energyml.rust.resqml import export_of, formats, geometry_of, mesh_of, properties_of

    pkg = Epc.open("model.epc")
    for one in pkg.find("TriangulatedSetRepresentation"):
        shape = geometry_of(pkg, one.uri)          # points and topology, placed
        points, triangles = mesh_of(pkg, one.uri)  # or one merged mesh
        values, problems = properties_of(pkg, one.uri)
        obj = export_of(pkg, [one.uri], "obj")      # the bytes, for a file or a response
        rich = export_of(pkg, [one.uri], "geojson", metadata=standard_metadata())

**Not a second copy of `energyml.resqml`.** An object crosses as a dictionary
and always will; what lives here are the operations *on* RESQML — the ones a
caller would otherwise redo from raw arrays.

These functions take the package rather than hanging off it, because a package
is a package: it is `energyml.rust.epc`'s job to hold objects and arrays, and
this module's to know what they mean. When WITSML and PRODML arrive they will
sit beside this one, and `Epc` will not have grown a method for either.
"""

from ._core import (
    export_of,
    formats,
    geometry_of,
    mesh_of,
    properties_of,
    standard_metadata,
)

__all__ = [
    "export_of",
    "formats",
    "geometry_of",
    "mesh_of",
    "properties_of",
    "standard_metadata",
]
