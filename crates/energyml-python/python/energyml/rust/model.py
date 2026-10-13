"""The generated model: what types exist, what they hold, how to make one.

    from energyml.rust.model import describe, template, types

    [one["qualified_type"] for one in types("resqml")][:3]
    # ['resqml20.obj_Activity', 'resqml20.obj_ActivityTemplate', …]

    describe("resqml22.TriangulatedSetRepresentation")["fields"]
    # [{'name': 'Citation', 'required': True, 'type': 'Object(…)', 'doc': '…'}, …]

    obj = template("resqml22.TriangulatedSetRepresentation", str(uuid.uuid4()))
    obj["Citation"]["Title"] = "A surface"
    pkg.put(obj)

**This is the generated model itself, read through reflection** — eleven
releases of RESQML, WITSML, PRODML and EML, with their fields, their
cardinalities and the schema's own documentation. A release added to the Rust
workspace shows up here the day it is registered, and nothing on this side
changes.

There are no Python classes, and that is the decision rather than an omission:
generating one class per type per release would be a second copy of
`crates/generated/` to keep in step, which is the trap both C++ and Python
RESQML libraries fell into. An object is a dictionary; `template` gives you one
with its `$type` set, `describe` says what to put in it, and `put` refuses
precisely — naming the field and the value — when something is wrong.

The uuid is **given, never invented**. `template(qt)` leaves it empty and `put`
refuses by naming it; `template(qt, uuid)` writes the one you pass, and refuses
at that call if it is not a uuid. Both forms stand, and setting `obj["uuid"]`
afterwards is the third — it is what you do when the identity comes from
somewhere else. Nothing defaults to a random one, because an identity is not
always a coin toss: `energyml-build` derives its uuids so that a second run over
the same source names the same objects, and a silent `uuid4()` would give such a
caller a fresh identity every run.
"""

from ._core import describe, template, types

__all__ = ["describe", "template", "types"]
