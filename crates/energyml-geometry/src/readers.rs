//! Which representations this build can turn into points, and how to add more.
//!
//! # Why a registry rather than a match
//!
//! Reading a representation used to be one `match` over five type names. That
//! works until somebody needs a sixth — and the somebody is not always in this
//! repository. An application with its own RESQML extension, a build that wants
//! IJK grids before this crate ships them, a version whose spelling nobody here
//! has seen: each of those is a reader to add, and none of them should require
//! editing the function every other reader goes through.
//!
//! So it is the same arrangement as `ArrayBackend` in the storage layer:
//! a trait, a registry, the built-ins pushed in by default, and anything the
//! caller adds tried **first**.
//!
//! ```no_run
//! # use energyml_geometry::readers::{Readers, RepresentationReader};
//! # fn example(mine: Box<dyn RepresentationReader>) {
//! let mut readers = Readers::builtin();
//! readers.push(mine);            // tried before the built-ins
//! # }
//! ```
//!
//! # What "another version" costs
//!
//! Nothing, in the ordinary case — and that is by design rather than by luck.
//! Readers are keyed on [`TypeInfo::type_key`], the name of the *concept*,
//! which the generator makes identical across versions: RESQML 2.0.1's
//! `obj_PointSetRepresentation` and RESQML 2.2's `PointSetRepresentation` are
//! both `PointSetRepresentation`. A new release that keeps the concept is read
//! by the reader that already exists.
//!
//! What costs something is a new release that *respells the insides* — and that
//! has happened twice already, both times silently: 2.2 dropped the patch
//! wrapper (`NodePatchGeometry` rather than `NodePatch` → `Geometry`), and
//! renamed a lattice's `Offset` to `Dimension`. Those live inside the readers,
//! which accept both spellings. A reader added here can do the same, or a
//! caller can push one that replaces it entirely.
//!
//! [`TypeInfo::type_key`]: energyml_core::reflect::TypeInfo::type_key

use energyml_core::value::Reflect;

use crate::error::Result;
use crate::geometry::Patch;
use crate::source::ValueSource;

/// Turns one kind of representation into patches of points.
///
/// Implement this to teach a build a representation it does not know. The
/// contract is the one the whole crate follows: **what cannot be evaluated is
/// an error naming it**, never an empty list of patches. A caller cannot tell
/// "nothing to draw" from "I did not understand this", and the difference
/// matters more than any individual reader.
pub trait RepresentationReader: Send + Sync {
    /// What this reader is, for diagnostics and for `energyml types`.
    fn name(&self) -> &'static str;

    /// Whether it reads this concept.
    ///
    /// The argument is the version-independent `type_key`, so a reader written
    /// once covers every version that keeps the concept.
    fn handles(&self, type_key: &str) -> bool;

    /// The patches, in the representation's own coordinate system.
    ///
    /// Placing them is [`crate::crs`]'s business, deliberately: a reader that
    /// projected its own points would make the choice for every caller.
    fn read(&self, representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>>;

    /// How many nodes the **document states**, without reading one array.
    ///
    /// The answer to *how big is this before I ask for it* — what a chooser
    /// needs to offer a thousand representations without opening the data file.
    /// [`read`](Self::read) is the only thing that knows how many points there
    /// really are, and it costs a decompression per dataset.
    ///
    /// # It is not the count `read` will produce, and must not be shown as one
    ///
    /// *Declared* and *drawn* are different quantities wherever the reader
    /// drops or derives nodes: a `Grid2dRepresentation` states its whole
    /// lattice and the reader keeps only the nodes that carry a location, an
    /// IJK grid states cells and is drawn by its skin. Two numbers agreeing on
    /// a total and disagreeing on what it means is the shape of bug this
    /// workspace keeps paying for, so the caller shows this one as *declared*
    /// and replaces it with the measured count the moment the object is read.
    ///
    /// # `None` is the ordinary answer, and it is not zero
    ///
    /// Most concepts state a node count nowhere: a `PolylineSetRepresentation`
    /// keeps its per-line node counts in an array, a `SubRepresentation` states
    /// elements rather than nodes. A reader that cannot say returns `None`; a
    /// zero would mean *this holds nothing*, which is a different claim and a
    /// false one.
    fn declared_nodes(&self, _representation: &dyn Reflect) -> Option<u64> {
        None
    }
}

/// The readers a build will try, in order.
pub struct Readers {
    entries: Vec<Box<dyn RepresentationReader>>,
}

impl Default for Readers {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Readers {
    /// No reader at all: every representation fails saying so.
    ///
    /// For a caller that wants only its own, with nothing inherited.
    pub fn none() -> Self {
        Self { entries: Vec::new() }
    }

    /// Everything this crate implements.
    pub fn builtin() -> Self {
        let mut readers = Self::none();
        for reader in crate::geometry::builtin_readers() {
            readers.entries.push(reader);
        }
        readers
    }

    /// Add one, tried **before** everything already there.
    ///
    /// Last pushed wins, so a caller can replace a built-in rather than only
    /// extend the set — which is what makes it possible to fix a reader in an
    /// application without waiting for a release here.
    pub fn push(&mut self, reader: Box<dyn RepresentationReader>) -> &mut Self {
        self.entries.insert(0, reader);
        self
    }

    /// The reader for a concept, if there is one.
    pub fn find(&self, type_key: &str) -> Option<&dyn RepresentationReader> {
        self.entries
            .iter()
            .find(|reader| reader.handles(type_key))
            .map(|reader| reader.as_ref())
    }

    /// Whether anything here reads this concept.
    pub fn handles(&self, type_key: &str) -> bool {
        self.find(type_key).is_some()
    }

    /// How many readers are registered.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// What the document states one representation holds, without a read.
    ///
    /// `None` when no reader handles the concept as well as when the reader
    /// that does cannot say — the caller has no use for the difference, since
    /// both mean *ask the geometry if you want a number*.
    pub fn declared_nodes(&self, representation: &dyn Reflect) -> Option<u64> {
        self.find(representation.type_info().type_key)
            .and_then(|reader| reader.declared_nodes(representation))
    }

    /// What each of them is called, in the order they are tried.
    pub fn names(&self) -> Vec<&'static str> {
        self.entries.iter().map(|reader| reader.name()).collect()
    }
}

/// The registry used when a caller does not supply one.
///
/// Public as [`default_registry`], for a caller that wants the built-ins
/// without building a second copy of them.
///
/// Built once. A caller that needs its own readers builds a [`Readers`] and
/// calls [`crate::geometry::of_with`]; nothing here is global mutable state,
/// which would make two parts of one program disagree about what can be read.
pub fn default_registry() -> &'static Readers {
    default_readers()
}

pub(crate) fn default_readers() -> &'static Readers {
    use std::sync::OnceLock;
    static READERS: OnceLock<Readers> = OnceLock::new();
    READERS.get_or_init(Readers::builtin)
}
