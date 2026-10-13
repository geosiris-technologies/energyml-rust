"""The abstraction above EPC and, one day, ETP.

    from energyml.rust.storage import Storage
    from energyml.rust.epc import Epc

    def count(store: Storage) -> int:       # works for any store
        return len(store.objects())

    count(Epc.open("model.epc"))

In Rust this is `EnergymlStorage`, the trait every store implements — and the
workspace's design decision ADR-0002 explains why it was drawn around
**ETP's requests** rather than around a zip file: an abstraction shaped by a
file has to be retorted the day a network transport arrives, while one shaped
by the protocol takes a file in its stride.

That decision belongs here too, which is why this module exists before there is
a second store to put in it. `Epc` is the first subclass; an ETP dataspace will
be the next, with the same method names and the same shapes of answer, and
nothing written against `Storage` will change.

Three things the trait's shape imposes, and they are why the methods look the
way they do:

- **addressing is by uri**, never by path — `eml:///resqml22.Foo(uuid)`;
- **the plural operations are the real ones**: `headers` and `get_objects` take
  lists, because ETP asks for a hundred objects in one message and an interface
  that could only ask for one would condemn the network store to be slow;
- **an answer is partial by nature**: one unreadable object out of a hundred
  must not cost the other ninety-nine, so a bad uri is missing from the answer
  rather than failing it.

What is *not* here: `save_as`, `create`, `set_data_root` and the rest of what
only a local package can do. Those live on `Epc`, because a dataspace has no
file to save.
"""

import abc

from .epc import Epc


class Storage(abc.ABC):
    """What every EnergyML store can answer.

    The Python face of `EnergymlStorage`. Methods are declared, never
    implemented: this is the shape of the answer, and the store gives it.
    """

    @property
    @abc.abstractmethod
    def store_kind(self) -> str:
        """`EPC` today; the name of whatever store this is."""

    @abc.abstractmethod
    def dataspaces(self) -> list:
        """The dataspaces it holds. An EPC is exactly one."""

    @abc.abstractmethod
    def objects(self) -> list:
        """Every object, as summaries: uri, uuid, qualified type, title."""

    @abc.abstractmethod
    def headers(self, uris: list) -> list:
        """The citations of these objects, without reading the objects."""

    @abc.abstractmethod
    def get(self, uri: str) -> dict:
        """One object, as a dictionary carrying its `$type`."""

    @abc.abstractmethod
    def put(self, obj) -> str:
        """Put an object; returns the uri written."""

    @abc.abstractmethod
    def remove(self, uri: str) -> None:
        """Remove an object."""

    @abc.abstractmethod
    def supported_types(self) -> list:
        """The types this store can read — the generated model behind it."""

    @abc.abstractmethod
    def arrays(self, uri: str) -> list:
        """The arrays an object names: a file, and a path inside it."""

    @abc.abstractmethod
    def array_shape(self, array) -> dict:
        """What an array weighs, without reading it."""

    @abc.abstractmethod
    def read_array(self, array):
        """An array's numbers, and its dimensions."""

    @abc.abstractmethod
    def read_array_part(self, array, start: list, count: list):
        """A slice of an array — what ETP calls a subarray."""

    @abc.abstractmethod
    def write_array(self, array, values, dimensions=None) -> None:
        """Write an array."""

    @abc.abstractmethod
    def references(self, uri: str) -> list:
        """What this object points at, and where inside it."""

    @abc.abstractmethod
    def referenced_by(self, uri: str) -> list:
        """Who points at this object."""

    @abc.abstractmethod
    def check(self, schema: bool = True, rules: bool = True) -> dict:
        """Everything that can be said against what this store holds."""


# **Registered rather than inherited.** `Epc` is a compiled type; making it
# derive from a Python class would mean building it through Python, which is a
# layer of indirection for nothing. `register` states the relationship, so
# `isinstance(pkg, Storage)` is true and a type checker follows it.
Storage.register(Epc)

# And the statement is checked rather than trusted: a method named above and
# missing below would be a promise this module makes and `Epc` does not keep —
# a documentation bug that only shows when somebody calls it. Here it is an
# ImportError, at once, naming the method.
_missing = sorted(name for name in Storage.__abstractmethods__ if not hasattr(Epc, name))
if _missing:  # pragma: no cover - a build error, not a runtime path
    raise ImportError(
        f"energyml.rust.storage declares {_missing} and energyml.rust.epc.Epc does not "
        "provide them: the abstraction and the store have drifted apart"
    )
del _missing

__all__ = ["Storage"]
