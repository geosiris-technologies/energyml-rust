//! Where the numbers come from, behind one trait.
//!
//! An EnergyML object points at its arrays; it does not hold them. The pointer
//! says *which file* and *which path inside it*, and something has to turn that
//! into bytes. That something is a backend, and it is a **trait** rather than a
//! hard-wired HDF5 call, for a reason set out in ADR-0002:
//!
//! - **HDF5 must be optional.** An EPC has to be readable, editable and
//!   writable with no native library anywhere near the build - that is what
//!   keeps `energyml-core` and this crate compiling for `wasm32`. Without the
//!   feature, only *reading an array* fails, and it fails saying which
//!   capability is missing rather than pretending the data is absent.
//! - **HDF5 does run in a browser**, through `h5wasm`, which is what makes
//!   H5Web work in JupyterLab and Galaxy. What does not exist is a *Rust* path
//!   to it. A WebAssembly application therefore supplies its **own**
//!   implementation of this trait, backed by h5wasm on the JavaScript side, and
//!   nothing in this crate changes. That is the whole point of the indirection.
//! - **Other formats exist.** CSV and Parquet appear behind the same
//!   `MimeType`-driven choice; the trait does not know about HDF5 either.
//!
//! # Where a file is
//!
//! An array pointer names a file, usually as a bare name - `model.h5` - which
//! the specification says sits beside the package. But a caller may know
//! better: a cloud service has neither the right names nor a disk, and a
//! package moved after export names files that are no longer there. So the
//! resolution is a small policy, [`ExternalFiles`], that the caller can steer.
//!
//! # The question is not "where is the file"
//!
//! It is **"which file holds this dataset"**, and the difference is the whole
//! design. A name is a *hint* - a good one, usually right, and worth trying
//! first. But a person who renames `model.epc` to `model_final.epc`, or who
//! receives the `.h5` from a colleague under another name, has not broken their
//! data: every number is still there, in a file sitting right next to the
//! package. Refusing them because a string no longer matches is refusing to
//! read data that is present, over bookkeeping.
//!
//! So [`ExternalFiles::candidates`] returns an **ordered list of places to
//! look**, and the caller walks it asking each one whether it actually holds
//! the dataset. The first that does, wins. The name decides the *order*; the
//! content decides the *answer*.
//!
//! The order, most authoritative first, is in [`Origin`].
//!
//! # Never quietly
//!
//! Reading a file the document did not name is exactly the mistake the uuid
//! cross-check in `EPC §3.2.3` exists to catch, and being permissive here is a
//! deliberate trade. It is paid for by saying so: every resolution that departs
//! from what the document wrote becomes a diagnostic naming what was asked for,
//! what was used, and why.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ByteOrder};
use crate::error::{Error, ExternalDataReason, Result};

/// Reads arrays out of one kind of external file.
///
/// Aligned on ETP protocol 9: metadata without the data, then the data, whole
/// or sliced.
pub trait ArrayBackend: Send + Sync {
    /// What this backend is, for diagnostics.
    fn name(&self) -> &'static str;

    /// Whether this backend handles a file, judged on the MIME type the object
    /// declared and on the file name.
    fn handles(&self, mime: Option<&str>, file: &Path) -> bool;

    /// Shape and element type, without reading the values -
    /// `GetDataArrayMetadata`.
    fn metadata(&self, file: &Path, path_in_file: &str) -> Result<ArrayMetadata>;

    /// The values, or the slice `start`/`count` names - `GetDataArrays` and
    /// `GetDataSubarrays`. Empty bounds mean the whole dataset.
    ///
    /// One copy: out of the file, into the returned block. Use
    /// [`ArrayBackend::read_into`] when even that allocation is one too many.
    fn read(&self, file: &Path, path_in_file: &str, start: &[i64], count: &[i64]) -> Result<ArrayData>;

    /// Decode straight into a buffer the caller already owns.
    ///
    /// **The zero-allocation path**, and the one a 3D application wants: the
    /// vertex buffer exists, the numbers go into it, and nothing else is
    /// touched. Returns the shape that was read.
    ///
    /// The buffer must hold exactly the elements the selection asks for, in the
    /// element type the dataset holds - [`ArrayBackend::metadata`] says both
    /// without reading anything. A mismatch is an error, never a silent partial
    /// read.
    fn read_into(
        &self,
        file: &Path,
        path_in_file: &str,
        start: &[i64],
        count: &[i64],
        out: &mut ArrayBuffer<'_>,
    ) -> Result<ArrayMetadata>;

    /// Which end of a multi-byte number this dataset writes first, when the
    /// backend can say and the width makes the question meaningful.
    ///
    /// `Ok(None)` for a backend that does not know, and for a one-byte element
    /// where there is nothing to order. **Never a guess**: a caller that writes
    /// this down - an EML 2.3 external array declares the storage type, byte
    /// order included - would otherwise make every value read wrong while the
    /// document stayed valid. A backend that cannot answer says so, and the
    /// caller refuses or defers rather than assuming the usual one.
    ///
    /// Separate from [`ArrayBackend::metadata`] because it is a separate
    /// question: the shape and the type are what a viewer needs before deciding
    /// whether it can afford the data, and byte order tells it nothing.
    fn byte_order(&self, _file: &Path, _path_in_file: &str) -> Result<Option<ByteOrder>> {
        Ok(None)
    }

    /// The uuid the file claims for itself, when the format has a place for one.
    ///
    /// EPC §3.2.3 requires an HDF5 file to carry, at its root, a `uuid`
    /// attribute equal to that of its `EpcExternalPartReference` - a
    /// cross-validation that catches the one mistake nothing else does: reading
    /// somebody else's file because it happened to have the right name.
    fn file_uuid(&self, _file: &Path) -> Result<Option<String>> {
        Ok(None)
    }

    /// Whether this backend can write, not only read.
    ///
    /// Asked before anything is attempted, so a caller can offer or refuse the
    /// operation rather than discovering halfway through that half a file has
    /// been created.
    fn writes(&self) -> bool {
        false
    }

    /// Create or replace a dataset, and the groups leading to it.
    ///
    /// The file is created when it does not exist. Replacing an existing
    /// dataset is allowed - HDF5 cannot resize one in place, so the old one is
    /// unlinked first and the space it held is **not** reclaimed. Rewriting the
    /// same file many times grows it; that is HDF5's model, not a defect here,
    /// and a caller that cares repacks.
    ///
    /// The default refuses by name. A backend that cannot write must say so
    /// rather than appear to succeed.
    fn write(&self, file: &Path, path_in_file: &str, _data: &ArrayData) -> Result<()> {
        Err(Error::ExternalData {
            location: format!("{}:{}", file.display(), path_in_file),
            reason: ExternalDataReason::Backend(format!(
                "the `{}` backend reads but does not write",
                self.name()
            )),
        })
    }

    /// Write several datasets into **one** file - what a plan of many arrays
    /// asks. One result per item, in their order: a refusal of one is not the
    /// refusal of the others.
    ///
    /// The default writes each by [`ArrayBackend::write`]. A backend whose cost
    /// is opening the file overrides it to open once: HDF5 spent 5.8 ms a
    /// dataset reopening and flushing a growing file, 55.6 s of the 58 s it took
    /// to write 9 576 small arrays for 1 300 SIGEOL boreholes (measured 28/09).
    fn write_many(&self, file: &Path, items: &[(&str, &ArrayData)]) -> Vec<Result<()>> {
        items
            .iter()
            .map(|(path, data)| self.write(file, path, data))
            .collect()
    }

    /// Put the uuid of the owning `EpcExternalPartReference` at the file root.
    ///
    /// The other half of the cross-validation [`ArrayBackend::file_uuid`]
    /// reads. Writing the numbers without writing this produces a file that
    /// this very crate will later refuse - so it is part of writing, not a
    /// nicety.
    fn set_file_uuid(&self, file: &Path, _uuid: &str) -> Result<()> {
        Err(Error::ExternalData {
            location: file.display().to_string(),
            reason: ExternalDataReason::Backend(format!(
                "the `{}` backend cannot stamp a file uuid",
                self.name()
            )),
        })
    }
}

/// How a file name in an array pointer becomes a path on a disk.
#[derive(Debug, Clone, Default)]
pub struct ExternalFiles {
    /// Where relative names are looked up - the folder holding the package,
    /// which is what the specification recommends.
    root: Option<PathBuf>,
    /// Names the caller has pinned to a path, whatever the package says.
    overrides: BTreeMap<String, PathBuf>,
    /// Files the caller offered **without saying what they are called**, in the
    /// order offered. See [`ExternalFiles::offer`].
    offered: Vec<PathBuf>,
    /// Names the caller says exist, with no path behind them at all. See
    /// [`ExternalFiles::declare_present`].
    present: Vec<String>,
}

/// Why a path is worth trying, and - through the order of the variants - how
/// strongly.
///
/// The list is a chain of responsibility: each origin is asked in turn whether
/// it can produce a file holding the dataset, and the first that can, answers.
/// What makes it useful rather than decorative is that the acceptance test is
/// the *content*, so a candidate that exists but does not hold the dataset
/// steps aside for the next one instead of failing the read.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Origin {
    /// The caller pinned this path to the name the document uses.
    ///
    /// First, because it is the only case where someone has stated, about
    /// *this* name, where the file is. Nothing the document says outranks it.
    Pinned,
    /// The caller offered this path without naming it, and this is its position
    /// in the order they were offered.
    ///
    /// Before the document's own name, because offering a file is an
    /// instruction - a person who passes `--h5 somewhere/data.h5` is saying
    /// "use this", and they usually pass it precisely because the package's own
    /// name is wrong.
    Offered(usize),
    /// The name the document wrote, resolved against the package's folder.
    ///
    /// What the specification means, and right almost always. It is third only
    /// because the two above are explicit human instructions.
    Declared,
    /// The file beside the package carrying the package's own name:
    /// `model.epc` → `model.h5`.
    Companion,
    /// Another file in the package's folder.
    ///
    /// The permissive tail, and the one that rescues a renamed pair. Tried in a
    /// stable order so that two runs over the same folder agree.
    Neighbour,
}

impl Origin {
    /// Whether using this needs saying out loud.
    ///
    /// [`Origin::Declared`] is the document being right, which is not news.
    /// Everything else means the read went somewhere the document did not
    /// point, and a caller that saves the package afterwards needs to know.
    pub fn is_a_departure(self) -> bool {
        self != Origin::Declared
    }

    /// What to call it in a message.
    pub fn describe(self) -> &'static str {
        match self {
            Origin::Pinned => "a file pinned to that name by the caller",
            Origin::Offered(_) => "a file the caller offered",
            Origin::Declared => "the file the document names",
            Origin::Companion => "the file beside the package with the package's name",
            Origin::Neighbour => "another file in the package's folder",
        }
    }
}

/// One place worth looking, and why.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Candidate {
    /// Why this path is being tried.
    pub origin: Origin,
    /// Where it is.
    pub path: PathBuf,
}

impl ExternalFiles {
    /// Resolve relative names against this folder.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
            overrides: BTreeMap::new(),
            offered: Vec::new(),
            present: Vec::new(),
        }
    }

    /// Pin one name to a path, overriding whatever the package says.
    ///
    /// For the packages whose HDF5 file was renamed, or moved, or never had the
    /// name the object claims - all of which happen.
    pub fn set(&mut self, name: impl Into<String>, path: impl Into<PathBuf>) -> &mut Self {
        self.overrides.insert(name.into(), path.into());
        self
    }

    /// Offer a file **without saying what it is called**.
    ///
    /// The difference from [`set`][Self::set] is the whole reason this exists.
    /// Pinning requires knowing the name the document uses, which is exactly
    /// what a person handed a renamed `.h5` does not know and should not have
    /// to find out by opening the `.rels` in a text editor. Offering says only
    /// "the numbers might be in here"; the search opens it and looks.
    ///
    /// Offers are tried in the order given, so a caller who knows which is
    /// likeliest can say so by putting it first.
    pub fn offer(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.offered.push(path.into());
        self
    }

    /// The files offered without a name, in order.
    pub fn offered(&self) -> &[PathBuf] {
        &self.offered
    }

    /// State that a file of this name is available, without saying where.
    ///
    /// **For a store that has no filesystem**, which in practice means a
    /// browser. Everything else here answers "which path", and every candidate
    /// is filtered on the path existing - which is false for all of them when
    /// there are no paths. Without this, the search yields nothing in a
    /// browser and no array is ever read, *whatever backend is plugged in*: the
    /// resolution would collapse silently one layer above the thing that was
    /// supposed to do the work.
    ///
    /// So a declared name is a candidate whose path is the name itself. It goes
    /// nowhere on disk, and it is not meant to: a backend that lives outside
    /// this process - h5wasm behind [`ArrayBackend`], holding the bytes in its
    /// own memory - receives that name and resolves it its own way.
    ///
    /// The bytes deliberately do **not** come through here. Whoever decodes the
    /// format is who should hold them, and for HDF5 in a browser that is the
    /// JavaScript side; copying nineteen megabytes into this process so that it
    /// can hand them straight back out would buy nothing.
    ///
    /// Named rather than offered, because a caller who has the file also knows
    /// what it is called - it is a file they picked or were given. A name that
    /// matches nothing in the package still gets its turn, under
    /// [`Origin::Offered`], in the order declared.
    pub fn declare_present(&mut self, name: impl Into<String>) -> &mut Self {
        let name = name.into();
        if !self.present.contains(&name) {
            self.present.push(name);
        }
        self
    }

    /// The names declared present, in order.
    pub fn present(&self) -> &[String] {
        &self.present
    }

    /// Whether any name was declared present - the shape a store takes when it
    /// has no filesystem to resolve against.
    pub fn is_virtual(&self) -> bool {
        !self.present.is_empty()
    }

    /// Every place worth looking for a dataset that the document says is in
    /// `declared`, most authoritative first.
    ///
    /// Existence is filtered here; **content is not**. Whether a candidate
    /// actually holds the dataset is a question only a backend can answer, and
    /// asking it is the caller's half of the job - see [`Origin`] for why the
    /// division matters.
    ///
    /// `package` is the `.epc` itself when there is one on disk, which is what
    /// [`Origin::Companion`] is derived from and what is excluded from
    /// [`Origin::Neighbour`].
    pub fn candidates(&self, declared: &str, package: Option<&Path>) -> Vec<Candidate> {
        let mut out: Vec<Candidate> = Vec::new();
        let push = |origin: Origin, path: PathBuf, out: &mut Vec<Candidate>| {
            if path.exists() && !out.iter().any(|c| c.path == path) {
                out.push(Candidate { origin, path });
            }
        };
        // A name someone declared present is a candidate as it stands: there is
        // no path to test, which is the whole point. Existence is asserted by
        // the caller and the *content* is still checked by the backend, so the
        // chain keeps the property that decides everything - the file that
        // holds the dataset wins, whether it lives on a disk or in another
        // process's memory.
        let declare = |origin: Origin, name: &str, out: &mut Vec<Candidate>| {
            let path = PathBuf::from(name);
            if !out.iter().any(|c| c.path == path) {
                out.push(Candidate { origin, path });
            }
        };
        let bare_declared = declared.rsplit(['/', '\\']).next().unwrap_or(declared);
        for (rank, name) in self.present.iter().enumerate() {
            let bare = name.rsplit(['/', '\\']).next().unwrap_or(name);
            // The one the document asks for comes first, then the rest in the
            // order declared - the same shape as pinned before offered.
            let origin = match name == declared || bare == bare_declared {
                true => Origin::Pinned,
                false => Origin::Offered(rank),
            };
            declare(origin, name, &mut out);
        }
        out.sort_by_key(|candidate| candidate.origin);

        // 1. Pinned by name, spelled either way: a package may write the whole
        //    URI or the bare file name for the same file.
        let bare = declared.rsplit(['/', '\\']).next().unwrap_or(declared);
        for key in [declared, bare] {
            if let Some(path) = self.overrides.get(key) {
                push(Origin::Pinned, path.clone(), &mut out);
            }
        }

        // 2. Offered, in the order offered.
        for (rank, path) in self.offered.iter().enumerate() {
            push(Origin::Offered(rank), path.clone(), &mut out);
        }

        // 3. What the document says, absolute or relative to the folder.
        if !declared.is_empty() {
            let written = Path::new(declared);
            if written.is_absolute() {
                push(Origin::Declared, written.to_path_buf(), &mut out);
            }
            if let Some(root) = &self.root {
                for relative in [declared, bare] {
                    push(Origin::Declared, root.join(relative), &mut out);
                }
            }
        }

        // 4. The package's own name with the other extension.
        if let Some(package) = package {
            push(Origin::Companion, package.with_extension("h5"), &mut out);
        }

        // 5. Everything else in the folder, in a stable order. Not filtered by
        //    extension - choosing a backend is the caller's job and it does it
        //    better, since a file named without one may still declare a MIME
        //    type - except for the one kind that is never an array file: a
        //    **package**. A folder holding two `.epc` files is the ordinary
        //    case, and offering one as a place to look for the other's numbers
        //    can only waste an open and confuse a message.
        if let Some(root) = &self.root
            && let Ok(entries) = std::fs::read_dir(root)
        {
            let mut neighbours: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .filter(|path| Some(path.as_path()) != package)
                .filter(|path| !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("epc")))
                .collect();
            neighbours.sort();
            for path in neighbours {
                push(Origin::Neighbour, path, &mut out);
            }
        }

        out
    }

    /// The folder relative names resolve against.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Set the folder relative names resolve against.
    pub fn set_root(&mut self, root: impl Into<PathBuf>) -> &mut Self {
        self.root = Some(root.into());
        self
    }

    /// Turn the name an array pointer carries into a path that exists.
    pub fn resolve(&self, name: &str) -> Result<PathBuf> {
        let missing = || Error::ExternalData {
            location: name.to_string(),
            reason: ExternalDataReason::FileMissing,
        };
        if name.is_empty() {
            return Err(missing());
        }
        // What the caller pinned wins, by whole name and by file name, since a
        // package may spell the same file either way.
        let bare = name.rsplit(['/', '\\']).next().unwrap_or(name);
        for key in [name, bare] {
            if let Some(path) = self.overrides.get(key) {
                return Ok(path.clone());
            }
        }

        let candidate = Path::new(name);
        if candidate.is_absolute() && candidate.exists() {
            return Ok(candidate.to_path_buf());
        }
        let root = self.root.as_deref().ok_or_else(missing)?;
        // The name as written, then its last segment: the schema suggests a URI
        // and fesapi writes a bare file name, and both occur.
        for relative in [name, bare] {
            let path = root.join(relative);
            if path.exists() {
                return Ok(path);
            }
        }
        Err(missing())
    }

    /// Where a file of this name *would* go, whether or not it is there yet.
    ///
    /// [`ExternalFiles::resolve`] answers "where is it", and every branch of it
    /// requires the file to exist - which is right for reading and useless for
    /// writing. This answers "where should it be written", using the same
    /// precedence so that a file created here is found by `resolve` afterwards.
    ///
    /// Errors when there is nowhere to put it: no override, not absolute, and
    /// no root. Inventing the working directory would scatter data files next
    /// to whatever the process happened to be run from.
    pub fn destination_for(&self, name: &str) -> Result<PathBuf> {
        let nowhere = || Error::ExternalData {
            location: name.to_string(),
            reason: ExternalDataReason::FileMissing,
        };
        if name.is_empty() {
            return Err(nowhere());
        }
        let bare = name.rsplit(['/', '\\']).next().unwrap_or(name);
        for key in [name, bare] {
            if let Some(path) = self.overrides.get(key) {
                return Ok(path.clone());
            }
        }
        let candidate = Path::new(name);
        if candidate.is_absolute() {
            return Ok(candidate.to_path_buf());
        }
        // Beside the package, which is where every real EPC keeps its arrays.
        self.root
            .as_deref()
            .map(|root| root.join(name))
            .ok_or_else(nowhere)
    }
}

/// The backends a store will try, in order.
#[derive(Default)]
pub struct Backends {
    entries: Vec<Box<dyn ArrayBackend>>,
}

impl Backends {
    /// No backend at all: every array read fails saying so.
    pub fn none() -> Self {
        Self::default()
    }

    /// Whatever this build was compiled with.
    ///
    /// HDF5 when the `hdf5` feature is on, nothing otherwise - and "nothing"
    /// is a supported state, not a broken one.
    pub fn compiled_in() -> Self {
        #[allow(unused_mut)]
        let mut backends = Self::default();
        #[cfg(feature = "hdf5")]
        backends.push(Box::new(crate::hdf5::Hdf5Backend));
        backends
    }

    /// Add a backend, tried before the ones already there.
    ///
    /// This is how a WebAssembly application plugs h5wasm in, and how CSV or
    /// Parquet will arrive.
    pub fn push(&mut self, backend: Box<dyn ArrayBackend>) -> &mut Self {
        self.entries.insert(0, backend);
        self
    }

    /// Whether anything at all can read an array.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The first backend that says it handles this file.
    pub fn find(&self, location: &ArrayLocation, file: &Path) -> Result<&dyn ArrayBackend> {
        self.entries
            .iter()
            .find(|backend| backend.handles(location.mime.as_deref(), file))
            .map(|backend| backend.as_ref())
            .ok_or_else(|| Error::ExternalData {
                location: format!("{}:{}", file.display(), location.path_in_file),
                reason: ExternalDataReason::FeatureNotEnabled {
                    feature: "hdf5",
                    mime: location.mime.clone(),
                },
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinned_file_wins_over_the_package() {
        let mut files = ExternalFiles::with_root("/nowhere");
        files.set("model.h5", "/elsewhere/renamed.h5");
        assert_eq!(
            files.resolve("model.h5").unwrap(),
            Path::new("/elsewhere/renamed.h5")
        );
        // Pinned by bare name, matched against a path-shaped reference.
        assert_eq!(
            files.resolve("sub/model.h5").unwrap(),
            Path::new("/elsewhere/renamed.h5")
        );
    }

    #[test]
    fn an_unknown_file_says_it_is_missing() {
        let files = ExternalFiles::with_root("/nowhere");
        assert!(matches!(
            files.resolve("absent.h5"),
            Err(Error::ExternalData {
                reason: ExternalDataReason::FileMissing,
                ..
            })
        ));
        assert!(files.resolve("").is_err(), "an empty name resolves to nothing");
    }

    #[test]
    fn without_a_backend_the_error_names_the_capability() {
        let backends = Backends::none();
        let location = ArrayLocation {
            file: "model.h5".to_string(),
            path_in_file: "/a/b".to_string(),
            mime: Some("application/x-hdf5".to_string()),
            start: Vec::new(),
            count: Vec::new(),
            proxy_uuid: None,
            field_path: "Points".to_string(),
        };
        match backends.find(&location, Path::new("model.h5")) {
            Err(Error::ExternalData {
                reason: ExternalDataReason::FeatureNotEnabled { feature, .. },
                ..
            }) => assert_eq!(feature, "hdf5"),
            Err(other) => panic!("wrong error: {other}"),
            Ok(backend) => panic!("no backend was registered, yet `{}` answered", backend.name()),
        }
    }
}
