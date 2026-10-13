"""EnergyML over the Rust workspace — `energyml.rust`.

One compiled extension (`_core`), arranged into modules named after what they
are about. The arrangement is deliberate: `energyml.resqml`, `energyml.witsml`
and friends are other distributions, and this one sits beside them rather than
in front of them.

Today:

- :mod:`energyml.rust.storage` — the abstraction above every store: what an
  EPC answers today and an ETP dataspace will answer tomorrow, with the same
  names. `Epc` is its first subclass.
- :mod:`energyml.rust.epc` — open, read, edit and write an EPC and its HDF5
  files, and ask its catalogue a question.
- :mod:`energyml.rust.model` — the generated model: which types exist, what
  fields they hold, and an empty object of one, ready to fill.
- :mod:`energyml.rust.resqml` — what the library makes of those objects:
  the shape of a representation, and the values hanging on it.

Planned, and named here so the names do not move later: ``operations``
(creating objects through `energyml-build`), ``witsml`` / ``prodml`` / ``eml``
(the same for their domains — units, property kinds, wells). None of them holds
generated classes: an object crosses as a dictionary and always will.

**Out of scope, and deliberately**: version translation and ETP. Both are
separate workshops in the Rust workspace, kept apart so that nothing else
depends on them, and this bridge keeps them out for the same reason — they
would arrive in the wheel, in the build time and in everyone's surface.

A module that does not exist yet is absent rather than empty: an empty module
that imports is indistinguishable from one that works and has nothing to say.
"""

from . import epc, model, resqml, storage

__all__ = ["epc", "model", "resqml", "storage"]
