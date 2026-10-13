"""An EPC package: its objects, its arrays, and the file it is written to.

    from energyml.rust.epc import Epc

    pkg = Epc.open("model.epc")
    for one in pkg.objects():
        print(one.qualified_type, one.title)

Everything here comes from the compiled extension; this module only says where
it lives. See `docs/python.md` in the repository for the worked examples.
"""

from ._core import ArrayRef, Epc, Summary

__all__ = ["ArrayRef", "Epc", "Summary"]
