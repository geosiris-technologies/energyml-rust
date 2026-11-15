//! Where referenced objects come from.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_storage::EpcStore;
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata};

use crate::error::Result;

/// Resolving a uuid to an object.
///
/// The whole reason CRS reading is not a pure function: an eml 2.3 coordinate
/// system is three data objects joined by references, so reading one means
/// asking the store for the other two.
///
/// One trait, so everything above is written once. An EPC file implements it
/// below; an ETP dataspace will implement it the same way and nothing else
/// changes - which is the point of the seam, not a side effect.
pub trait ObjectSource {
    /// An object of the store, by uuid. `None` when it holds no such object.
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>>;

    /// The same, **saying why** when there is nothing to hand back.
    ///
    /// A reader resolving a reference does not care why the target is absent -
    /// it has one thing to do either way. A reader walking a whole package
    /// does: *this part breaks its schema* and *this package holds no such
    /// object* are repaired in different places, and the first is the message
    /// that names the switch (`--lenient`, `--strict`) which would help.
    ///
    /// Provided rather than required, so a source with nothing more to say
    /// implements nothing; the ones that know the reason override it.
    fn read_object(&self, uuid: &str) -> std::result::Result<Box<dyn DynObject>, String> {
        self.object(uuid)
            .ok_or_else(|| "this store holds no such object".to_string())
    }

    /// An object by its **uri** - the address a caller holds when it has one.
    ///
    /// Two versions of an object share a uuid, and only the uri tells them
    /// apart (ADR-0003, ADR-0010): a store that
    /// can hold both overrides this with its own resolution, and answers the
    /// version the uri names or refuses by name. The default is for a source
    /// that holds one object per uuid - for it the uuid **is** the address, so
    /// it is asked by the uuid the uri carries.
    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        self.object(&energyml_core::identity::parse_address(uri).0)
    }

    /// [`ObjectSource::object_at`], saying why when there is nothing.
    fn read_object_at(&self, uri: &str) -> std::result::Result<Box<dyn DynObject>, String> {
        self.read_object(&energyml_core::identity::parse_address(uri).0)
    }

    /// **The object a reference names** - a `DataObjectReference`, a
    /// `ContactElementReference`, a `ContactElement` - by its full identity
    /// ([`energyml_core::identity::address_of_reflect`]): the version it names is
    /// the version read, and a reference that names no type is followed by its
    /// uuid, the only address it has. The one way a reader follows a reference.
    fn object_of(&self, reference: &dyn Reflect) -> Option<Box<dyn DynObject>> {
        self.object_at(&energyml_core::identity::address_of_reflect(reference)?)
    }

    /// [`ObjectSource::object_of`], saying why when there is nothing.
    fn read_object_of(&self, reference: &dyn Reflect) -> std::result::Result<Box<dyn DynObject>, String> {
        let uri = energyml_core::identity::address_of_reflect(reference)
            .ok_or_else(|| format!("a `{}` that names no object", reference.type_info().name))?;
        self.read_object_at(&uri)
    }
}

impl ObjectSource for EpcStore {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.get_object(uuid).ok()
    }

    fn read_object(&self, uuid: &str) -> std::result::Result<Box<dyn DynObject>, String> {
        self.get_object(uuid).map_err(|why| format!("{why}"))
    }

    // The store resolves a uri itself - the version it names included.
    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        self.get_object(uri).ok()
    }

    fn read_object_at(&self, uri: &str) -> std::result::Result<Box<dyn DynObject>, String> {
        self.get_object(uri).map_err(|why| format!("{why}"))
    }
}

/// Reading the arrays an object points at.
///
/// Sits on top of [`ObjectSource`] because a derived array - a boolean written
/// as the indices that are true, an integer array written as a mask - needs
/// both: the numbers, and sometimes the object holding them.
///
/// One trait for both EPC and ETP, again: the layers above name this and
/// nothing else, so the transport is a decision made once, where the store is
/// built.
pub trait ArraySource: ObjectSource {
    /// Every external part this array points at, resolved to a file.
    ///
    /// A list rather than one: eml 2.3 lets a single logical array be spread
    /// over several `ExternalDataArrayPart`s.
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation>;

    /// Shape and element type, without reading the values.
    fn metadata(&self, location: &ArrayLocation) -> Result<ArrayMetadata>;

    /// Read one part.
    fn read(&self, location: &ArrayLocation) -> Result<ArrayData>;

    /// Read one part into a buffer the caller already owns.
    fn read_into(&self, location: &ArrayLocation, out: &mut ArrayBuffer<'_>) -> Result<ArrayMetadata>;
}

impl ArraySource for EpcStore {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        self.arrays_of(object)
    }

    fn metadata(&self, location: &ArrayLocation) -> Result<ArrayMetadata> {
        Ok(EpcStore::array_metadata(self, location)?)
    }

    fn read(&self, location: &ArrayLocation) -> Result<ArrayData> {
        Ok(EpcStore::read_array(self, location)?)
    }

    fn read_into(&self, location: &ArrayLocation, out: &mut ArrayBuffer<'_>) -> Result<ArrayMetadata> {
        Ok(EpcStore::read_array_into(self, location, out)?)
    }
}

/// A source that resolves nothing.
///
/// For reading a self-contained RESQML 2.0.1 CRS, where following a reference
/// is never needed. An eml 2.3 compound CRS read through this comes back
/// missing its offsets and its EPSG code - which is why
/// [`crate::crs::read`] says so in its documentation.
pub struct NoObjects;

impl ObjectSource for NoObjects {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

impl ArraySource for NoObjects {
    fn locate(&self, _object: &dyn Reflect) -> Vec<ArrayLocation> {
        Vec::new()
    }
    fn metadata(&self, _location: &ArrayLocation) -> Result<ArrayMetadata> {
        Err(crate::Error::unsupported(
            "reading an array",
            "this source holds none",
        ))
    }
    fn read(&self, _location: &ArrayLocation) -> Result<ArrayData> {
        Err(crate::Error::unsupported(
            "reading an array",
            "this source holds none",
        ))
    }
    fn read_into(&self, _l: &ArrayLocation, _o: &mut ArrayBuffer<'_>) -> Result<ArrayMetadata> {
        Err(crate::Error::unsupported(
            "reading an array",
            "this source holds none",
        ))
    }
}

// ---------------------------------------------------------------------------
// The bridge an asynchronous store crosses to reach these traits
// ---------------------------------------------------------------------------

/// Objects and arrays pulled out of a store in advance, served synchronously.
///
/// # The problem this exists for
///
/// [`ObjectSource`] and [`ArraySource`] are **synchronous**, and everything
/// above them - colour resolution, property reading, the geometry readers -
/// is written against them. [`EnergymlStorage`](energyml_storage::storage::EnergymlStorage)
/// is **asynchronous**, because ETP is a WebSocket and a browser cannot block.
/// An `EpcStore` bridges the two by having a complete synchronous API of its
/// own; a network client has nothing to bridge with, and blocking on a future
/// is exactly what wasm forbids.
///
/// So the bridge is not a blocking call but a **prefetch**: ask for everything
/// in one asynchronous pass, then read it synchronously. That is the shape ETP
/// was designed for in the first place - every request is plural, and asking
/// for a hundred objects in one message is the point.
///
/// # What it does not do
///
/// It does not guess. [`Prefetched::fetch`] follows references transitively to
/// a depth the caller states, because a colour cascade walks
/// property → kind → parents → representation → interpretation → feature and a
/// CRS walk in eml 2.3 is three objects deep. Arrays are **located** but not
/// read, because reading every array of a package is not a prefetch, it is a
/// download; [`Prefetched::fetch_arrays`] takes the ones the caller wants.
/// Anything that was not fetched is refused **by name** rather than returned
/// empty - a reader that silently sees no array draws a representation with no
/// points and says nothing.
pub struct Prefetched {
    /// What it holds, by lowercase uuid: **every version** of an object, in
    /// insertion order, never one over another. The identity of an entry is its
    /// uuid and its object version (the uri, without the dataspace): a uri names
    /// the version it names ([`ObjectSource::object_at`]), and a bare uuid is
    /// only the entry adapter, which answers the version inserted last.
    objects: std::collections::HashMap<String, Vec<Held>>,
    /// Array data by the identity of its location.
    arrays: std::collections::HashMap<String, ArrayData>,
}

/// One version of one object, with the array locations it declared.
struct Held {
    /// The object version, trimmed; `None` when the object declares none.
    version: Option<String>,
    object: Box<dyn DynObject>,
    locations: Vec<ArrayLocation>,
}

/// A version as an identity keeps it: trimmed, and `None` when blank.
fn named_version(version: Option<&str>) -> Option<String> {
    version
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_string)
}

/// What identifies one array read: everything the store needs to perform it.
///
/// Not the field path - two fields may name the same dataset, and a caller may
/// legitimately locate an array twice. Not a hash either: the pieces are short
/// and the string is only ever a map key.
fn slot(location: &ArrayLocation) -> String {
    format!(
        "{}|{}|{:?}|{:?}",
        location.file, location.path_in_file, location.start, location.count
    )
}

impl Default for Prefetched {
    fn default() -> Self {
        Self::new()
    }
}

impl Prefetched {
    /// An empty cache, to be filled by hand.
    pub fn new() -> Self {
        Prefetched {
            objects: std::collections::HashMap::new(),
            arrays: std::collections::HashMap::new(),
        }
    }

    /// How many objects it holds - each version of an object counts.
    pub fn len(&self) -> usize {
        self.objects.values().map(Vec::len).sum()
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Every uuid it holds, lowercase.
    pub fn uuids(&self) -> Vec<String> {
        let mut out: Vec<String> = self.objects.keys().cloned().collect();
        out.sort();
        out
    }

    /// The version of an object that `uuid` and `wanted` name: the version
    /// named, else one that names none or that the object does not contradict
    /// ([`energyml_core::identity::version_fit`]); the one inserted last among
    /// equals. `None` when every version held contradicts the one wanted - the
    /// refusal is by name, never another version's object.
    fn pick(&self, uuid: &str, wanted: Option<&str>) -> Option<&Held> {
        use energyml_core::identity::{VersionFit, version_fit};
        let held = self.objects.get(&uuid.to_ascii_lowercase())?;
        let mut best: Option<(VersionFit, &Held)> = None;
        for one in held {
            let fit = version_fit(wanted, one.version.as_deref());
            if fit == VersionFit::Contradicts {
                continue;
            }
            // `<=`: a later insertion wins a tie.
            if best.is_none_or(|(known, _)| fit <= known) {
                best = Some((fit, one));
            }
        }
        best.map(|(_, one)| one)
    }

    /// The array locations recorded for one object, by uuid: those of the
    /// version inserted last. [`Prefetched::locations_at`] is the one that names
    /// the version.
    pub fn locations_of(&self, uuid: &str) -> &[ArrayLocation] {
        self.objects
            .get(&uuid.to_ascii_lowercase())
            .and_then(|held| held.last())
            .map(|one| one.locations.as_slice())
            .unwrap_or_default()
    }

    /// The array locations recorded for the object a uri names, the version it
    /// names included. Empty when the cache holds no such object.
    pub fn locations_at(&self, uri: &str) -> &[ArrayLocation] {
        let (uuid, version) = energyml_core::identity::parse_address(uri);
        self.pick(&uuid, version.as_deref())
            .map(|one| one.locations.as_slice())
            .unwrap_or_default()
    }

    /// Every location recorded, across every object and every version.
    pub fn locations(&self) -> Vec<ArrayLocation> {
        let mut keys: Vec<&String> = self.objects.keys().collect();
        keys.sort();
        keys.into_iter()
            .flat_map(|uuid| {
                self.objects[uuid]
                    .iter()
                    .flat_map(|one| one.locations.iter().cloned())
            })
            .collect()
    }

    /// Put one object in, and record the arrays it declares.
    ///
    /// An object is identified by its uuid **and its object version**: inserting
    /// a second version of an object keeps both, and inserting the same version
    /// again replaces it.
    ///
    /// `locate` is how the store resolves an `EpcExternalPartReference`; pass
    /// the store's own [`energyml_storage::storage::EnergymlStorage::arrays_of`].
    pub fn insert(
        &mut self,
        object: Box<dyn DynObject>,
        locate: &dyn Fn(&dyn Reflect) -> Vec<ArrayLocation>,
    ) {
        let Some(uuid) = energyml_core::identity::uuid_of_reflect(object.as_reflect()) else {
            // An object with no uuid cannot be reached by reference, so
            // caching it would be caching something nothing can ask for.
            return;
        };
        let uuid = uuid.to_ascii_lowercase();
        let version = named_version(energyml_core::identity::object_version_of_reflect(
            object.as_reflect(),
        ));
        let locations = locate(object.as_reflect());
        let held = self.objects.entry(uuid).or_default();
        held.retain(|one| one.version != version);
        held.push(Held {
            version,
            object,
            locations,
        });
    }

    /// Put the data of one array in.
    pub fn insert_array(&mut self, location: &ArrayLocation, data: ArrayData) {
        self.arrays.insert(slot(location), data);
    }

    /// Fill from a store, following references `depth` hops out from `roots`.
    ///
    /// `roots` are URIs. `depth` is how far a reference is followed:
    ///
    /// - `0` - the roots and nothing else.
    /// - `1` - enough for a representation and the objects it names directly.
    /// - `3` - enough for a colour cascade
    ///   (representation → interpretation → feature) and for an eml 2.3
    ///   compound CRS.
    ///
    /// Only references to objects the store actually lists are followed: a
    /// dangling reference is a fact about the package, not a reason to fail a
    /// prefetch. An object that does not deserialize is skipped for the same
    /// reason the plural reads are partial - one bad object must not lose the
    /// other ninety-nine.
    ///
    /// Arrays are located, not read. See [`Prefetched::fetch_arrays`].
    pub async fn fetch<S>(store: &S, roots: &[String], depth: usize) -> Result<Self>
    where
        S: energyml_storage::storage::EnergymlStorage + ?Sized,
    {
        let inventory = store.resources().await?;
        // uuid → every URI of the inventory under it, with its version: a
        // reference names a uuid (and a version, when it carries one) and the
        // store is asked by URI. Two versions of one uuid are two entries, so a
        // reference to either is followed to the right one.
        let mut by_uuid: std::collections::HashMap<String, Vec<(Option<String>, String)>> =
            std::collections::HashMap::new();
        for summary in &inventory {
            // The version is the one the uri names; a uri that is no ETP uri
            // names none (`parse_address` reads a bare text as a uuid alone).
            let version = energyml_core::identity::parse_address(&summary.uri).1;
            by_uuid
                .entry(summary.uuid.to_ascii_lowercase())
                .or_default()
                .push((named_version(version.as_deref()), summary.uri.clone()));
        }

        let mut cache = Prefetched::new();
        let mut frontier: Vec<String> = roots.to_vec();
        let mut asked: std::collections::HashSet<String> = frontier.iter().cloned().collect();

        for _ in 0..=depth {
            if frontier.is_empty() {
                break;
            }
            let fetched = store.get_objects(&frontier).await;
            let mut next: Vec<String> = Vec::new();
            for object in fetched.into_iter().flatten() {
                for edge in energyml_core::identity::collect_refs(object.as_reflect()) {
                    if edge.target.uuid.is_empty() {
                        continue;
                    }
                    let Some(entries) = by_uuid.get(&edge.target.uuid.to_ascii_lowercase()) else {
                        continue;
                    };
                    // The versions the reference names - or, when it names
                    // none, every one of them: a short reference contradicts
                    // nothing, and the cache answers the uri it is later asked.
                    use energyml_core::identity::{VersionFit, version_fit};
                    let wanted = edge.target.object_version;
                    let best = entries
                        .iter()
                        .map(|(version, _)| version_fit(wanted, version.as_deref()))
                        .min();
                    let Some(best) = best.filter(|fit| *fit != VersionFit::Contradicts) else {
                        continue;
                    };
                    for (version, uri) in entries {
                        if version_fit(wanted, version.as_deref()) == best && asked.insert(uri.clone()) {
                            next.push(uri.clone());
                        }
                    }
                }
                cache.insert(object, &|reflected| store.arrays_of(reflected));
            }
            frontier = next;
        }
        Ok(cache)
    }

    /// Read the data of the given locations into the cache.
    ///
    /// Separate from [`Prefetched::fetch`] because reading every array of a
    /// package is a download, not a prefetch. A caller that knows it wants the
    /// points of one representation passes `locations_of(uuid)`; a caller that
    /// wants the lot passes [`Prefetched::locations`].
    ///
    /// Returns the number read. An array that fails to read is left out rather
    /// than failing the batch - [`ArraySource::read`] will then refuse it by
    /// name, which is more useful than a prefetch that fails wholesale.
    pub async fn fetch_arrays<S>(&mut self, store: &S, locations: &[ArrayLocation]) -> usize
    where
        S: energyml_storage::storage::EnergymlStorage + ?Sized,
    {
        let mut read = 0usize;
        for location in locations {
            if self.arrays.contains_key(&slot(location)) {
                continue;
            }
            if let Ok(data) = store.read_array(location).await {
                self.arrays.insert(slot(location), data);
                read += 1;
            }
        }
        read
    }
}

impl ObjectSource for Prefetched {
    /// By uuid: the version inserted last. A caller holding a uri asks
    /// [`ObjectSource::object_at`], which names the version.
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.objects
            .get(&uuid.to_ascii_lowercase())
            .and_then(|held| held.last())
            .map(|one| one.object.clone_box())
    }

    /// The version the uri names, or one that contradicts nothing when it names
    /// none; `None` when the cache holds only other versions.
    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        let (uuid, version) = energyml_core::identity::parse_address(uri);
        self.pick(&uuid, version.as_deref())
            .map(|one| one.object.clone_box())
    }

    fn read_object_at(&self, uri: &str) -> std::result::Result<Box<dyn DynObject>, String> {
        let (uuid, version) = energyml_core::identity::parse_address(uri);
        self.object_at(uri).ok_or_else(|| match version {
            Some(version) if self.objects.contains_key(&uuid) => {
                format!("this cache holds object `{uuid}`, but not at version `{version}`")
            }
            _ => "this cache holds no such object".to_string(),
        })
    }
}

impl ArraySource for Prefetched {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        // From what the prefetch recorded, not by walking the object again:
        // walking it here would produce **unresolved** locations, since this
        // cache has no idea what an `EpcExternalPartReference` stands for.
        // That is the whole reason `fetch` records them at insertion time.
        if let Some(uuid) = energyml_core::identity::uuid_of_reflect(object) {
            // The version the object declares: two versions of one object do
            // not share their locations.
            return self
                .pick(uuid, energyml_core::identity::object_version_of_reflect(object))
                .map(|one| one.locations.clone())
                .unwrap_or_default();
        }
        // A part of an object, not the object - what every value reader hands
        // over (`source.locate(array)`), and it has no uuid to look up. Its
        // arrays are among those recorded for the object it belongs to: walk
        // the part without resolving anything, and take the recorded
        // location with the same path, proxy and slice. Found 15/09 by the
        // ETP bridge's acceptance test - until then no reader had gone
        // through a prefetch, and each would have drawn nothing.
        energyml_storage::array::locate(object)
            .into_iter()
            .filter_map(|found| {
                self.objects
                    .values()
                    .flatten()
                    .flat_map(|one| one.locations.iter())
                    .find(|recorded| {
                        recorded.path_in_file == found.path_in_file
                            && recorded.proxy_uuid == found.proxy_uuid
                            && recorded.start == found.start
                            && recorded.count == found.count
                    })
                    .cloned()
            })
            .collect()
    }

    fn metadata(&self, location: &ArrayLocation) -> Result<ArrayMetadata> {
        match self.arrays.get(&slot(location)) {
            Some(data) => Ok(data.metadata.clone()),
            None => Err(not_prefetched(location)),
        }
    }

    fn read(&self, location: &ArrayLocation) -> Result<ArrayData> {
        match self.arrays.get(&slot(location)) {
            Some(data) => Ok(data.clone()),
            None => Err(not_prefetched(location)),
        }
    }

    fn read_into(&self, location: &ArrayLocation, out: &mut ArrayBuffer<'_>) -> Result<ArrayMetadata> {
        use energyml_storage::array::ArrayValues;

        let data = self.read(location)?;
        // The typed copy, not a byte copy: a buffer of `f64` handed an array of
        // `i32` would take the bytes and read numbers that were never there.
        // The element types have to meet, and a refusal has to say so.
        macro_rules! fill {
            ($values:expr, $room:expr) => {{
                if $room.len() < $values.len() {
                    return Err(crate::Error::unsupported(
                        "reading into a buffer",
                        format!(
                            "the buffer takes {} element(s) and the array has {}",
                            $room.len(),
                            $values.len()
                        ),
                    ));
                }
                $room[..$values.len()].copy_from_slice($values);
            }};
        }
        match (&data.values, out) {
            (ArrayValues::F64(v), ArrayBuffer::F64(room)) => fill!(v, room),
            (ArrayValues::F32(v), ArrayBuffer::F32(room)) => fill!(v, room),
            (ArrayValues::I64(v), ArrayBuffer::I64(room)) => fill!(v, room),
            (ArrayValues::I32(v), ArrayBuffer::I32(room)) => fill!(v, room),
            (ArrayValues::U64(v), ArrayBuffer::U64(room)) => fill!(v, room),
            (ArrayValues::U32(v), ArrayBuffer::U32(room)) => fill!(v, room),
            (ArrayValues::U8(v), ArrayBuffer::U8(room)) => fill!(v, room),
            (_, room) => {
                return Err(crate::Error::unsupported(
                    "reading into a buffer",
                    format!(
                        "the array holds {} and the buffer takes {}",
                        data.metadata.element_type,
                        room.element_type()
                    ),
                ));
            }
        }
        Ok(data.metadata)
    }
}

/// Refused by name: which array, and what to do about it.
fn not_prefetched(location: &ArrayLocation) -> crate::Error {
    crate::Error::unsupported(
        "reading an array",
        format!(
            "`{}` of `{}` was not prefetched - add it to `fetch_arrays` before reading",
            location.field_path, location.path_in_file
        ),
    )
}

/// An object named by a uri written as a value (a cell of a table, a field of a
/// document): what the uri names, and whether the package holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectChip {
    /// The uri, trimmed, as written.
    pub uri: String,
    /// The object's qualified type: the object's own when it is held, else the one the uri names.
    pub qualified_type: Option<String>,
    /// `Citation.Title`, when the object is held.
    pub title: Option<String>,
    /// Where the named object is.
    pub presence: Presence,
}

/// Where the named object is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    /// The source holds it.
    InPackage,
    /// The uri names an object of this dataspace that the source does not hold.
    NotInPackage,
    /// The uri names another dataspace than the one `objects` stands for: not looked up.
    OtherDataspace {
        /// The dataspace the uri names.
        dataspace: String,
    },
}

/// Why a value is no object uri.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UriRefusal {
    /// The value does not match the ETP uri grammar.
    #[error("`{value}` is not an ETP uri")]
    NotAnEtpUri {
        /// The value as written.
        value: String,
    },
    /// An ETP uri of a dataspace, which names no object.
    #[error("`{value}` names a dataspace, not an object")]
    NotAnObject {
        /// The value as written.
        value: String,
    },
}

/// What a uri written as a value names: the object chip.
///
/// `here` is the dataspace `objects` stands for (`None`: the default dataspace, which an
/// EPC is). A uri of another dataspace is **not looked up** (`OtherDataspace`): looking it up
/// here would answer "absent" for an object that lives elsewhere. A held object is found by
/// [`ObjectSource::object_at`], the one door of an object by uri; the cost is one read of the
/// object, so the caller keeps the chip.
///
/// # Errors
///
/// [`UriRefusal::NotAnEtpUri`] for a value that is not an ETP uri, [`UriRefusal::NotAnObject`] for
/// a dataspace uri (ETP v1.2 section 25.3.6).
pub fn object_chip(
    value: &str,
    here: Option<&str>,
    objects: &dyn ObjectSource,
) -> std::result::Result<ObjectChip, UriRefusal> {
    let value = value.trim();
    let uri = energyml_core::identifiers::EtpUri::parse(value).map_err(|_| UriRefusal::NotAnEtpUri {
        value: value.to_string(),
    })?;
    if uri.uuid.is_none() {
        return Err(UriRefusal::NotAnObject {
            value: value.to_string(),
        });
    }
    let named_type = uri.qualified_type().map(|qualified| qualified.to_string());
    if uri.dataspace.as_deref() != here {
        return Ok(ObjectChip {
            uri: value.to_string(),
            qualified_type: named_type,
            title: None,
            presence: Presence::OtherDataspace {
                dataspace: uri.dataspace.clone().unwrap_or_default(),
            },
        });
    }
    match objects.object_at(value) {
        Some(object) => {
            let reflect = object.as_reflect();
            Ok(ObjectChip {
                uri: value.to_string(),
                qualified_type: reflect
                    .type_info()
                    .qualified_type
                    .map(str::to_string)
                    .or(named_type),
                title: energyml_core::identity::title_of_reflect(reflect).map(str::to_string),
                presence: Presence::InPackage,
            })
        }
        None => Ok(ObjectChip {
            uri: value.to_string(),
            qualified_type: named_type,
            title: None,
            presence: Presence::NotInPackage,
        }),
    }
}
