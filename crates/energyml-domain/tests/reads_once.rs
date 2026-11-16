//! **An object is read once per operation** (review of 15/11, lot C1): what an operation reads of the package
//! it keeps, instead of fetching and parsing it again for each property, frame or table that needs it.
//!
//! Every test counts the reads an [`ObjectSource`] is asked for, by a source that counts. Nothing is read
//! from a file: the objects are built in memory.
//!
//! A cache never serves a stale answer here because each lives no longer than the operation that made it
//! (a `Resolver`, a `Cascade`, a `StrataCache`, one `integrity::run`): the doc of each says so.

use std::cell::RefCell;
use std::collections::HashMap;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::color::{Cascade, Entry, GraphicalSet, Information, Request, Scaling, Scope};
use energyml_domain::integrity;
use energyml_domain::property::PropertyKindRef;
use energyml_domain::property_kind::Resolver;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_domain::table::IDENTIFIER;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata};

use crate::integrity::{Package, boxed, citation};

/// An [`ObjectSource`] that counts every object it is asked for, by uuid (lower case), however the caller asked
/// (by uuid or by uri).
pub(crate) struct Counting<'a> {
    inner: &'a dyn ArraySource,
    reads: RefCell<HashMap<String, usize>>,
}

impl<'a> Counting<'a> {
    pub(crate) fn new(inner: &'a dyn ArraySource) -> Self {
        Counting {
            inner,
            reads: RefCell::default(),
        }
    }

    fn count(&self, address: &str) {
        let uuid = energyml_core::identity::parse_address(address)
            .0
            .to_ascii_lowercase();
        *self.reads.borrow_mut().entry(uuid).or_default() += 1;
    }

    /// How many times the object with this uuid was asked for.
    pub(crate) fn reads_of(&self, uuid: &str) -> usize {
        self.reads
            .borrow()
            .get(&uuid.to_ascii_lowercase())
            .copied()
            .unwrap_or(0)
    }
}

impl ObjectSource for Counting<'_> {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.count(uuid);
        self.inner.object(uuid)
    }
    fn read_object(&self, uuid: &str) -> Result<Box<dyn DynObject>, String> {
        self.count(uuid);
        self.inner.read_object(uuid)
    }
    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        self.count(uri);
        self.inner.object_at(uri)
    }
    fn read_object_at(&self, uri: &str) -> Result<Box<dyn DynObject>, String> {
        self.count(uri);
        self.inner.read_object_at(uri)
    }
}

impl ArraySource for Counting<'_> {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        self.inner.locate(object)
    }
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        self.inner.metadata(location)
    }
    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        self.inner.read(location)
    }
    fn read_into(
        &self,
        location: &ArrayLocation,
        out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        self.inner.read_into(location, out)
    }
}

const KIND: &str = "00000000-0000-0000-0000-00000000c101";

/// A kind of the package shared by many properties is fetched and parsed once by a resolver, however many
/// properties ask for it - and the chain it climbs with it.
///
/// Break it: in `Resolver::lookup`, read through `self.objects` directly instead of `self.package_kind`; the kind
/// is then read once per property and this fails.
#[test]
fn a_property_kind_is_read_once_by_a_resolver() {
    let package = Package(vec![(
        KIND.to_string(),
        boxed(eml23::PropertyKind {
            uuid: KIND.to_string(),
            citation: citation("a kind of the package"),
            ..Default::default()
        }),
    )]);
    let counting = Counting::new(&package);
    let resolver = Resolver::new(&counting);
    let reference = PropertyKindRef::Object {
        uuid: KIND.to_string(),
        title: None,
    };
    for _ in 0..5 {
        let resolution = resolver.resolve(&reference).expect("the kind is in the package");
        assert_eq!(resolution.kind().title, "a kind of the package");
    }
    assert_eq!(
        counting.reads_of(KIND),
        1,
        "five properties, one read of their kind"
    );
    // A kind the package does not hold is remembered as absent too: it is asked once, and the dictionary answers.
    let absent = PropertyKindRef::Object {
        uuid: "00000000-0000-0000-0000-00000000c1ff".to_string(),
        title: None,
    };
    for _ in 0..3 {
        let _ = resolver.resolve(&absent);
    }
    assert_eq!(counting.reads_of("00000000-0000-0000-0000-00000000c1ff"), 1);
}

const A: &str = "00000000-0000-0000-0000-00000000c201";
const B: &str = "00000000-0000-0000-0000-00000000c202";

fn table_uri(uuid: &str) -> String {
    format!("eml:///eml23.ColumnBasedTable({uuid})")
}

fn text_column(title: &str, values: &[&str]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        property_kind: eml23::DataObjectReference {
            uuid: IDENTIFIER.to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: "a kind".to_string(),
            ..Default::default()
        },
        values: Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
            values: values.iter().map(|value| value.to_string()).collect(),
            count_per_value: 1,
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// The two link checks judge one reading of a table and its target: the target table is read **once per run**,
/// not once per check.
///
/// Break it: make `check_table_links` build its reading without looking in `context.memory.checked`; the target
/// is then read by each of the two checks and this fails.
#[test]
fn the_target_of_a_link_is_read_once_by_the_two_link_checks() {
    let mut linking = text_column("index_uid", &["i1", "i1"]);
    linking.facet.push(eml23::PropertyKindFacet {
        facet: eml23::FacetExt::Other(table_uri(B)),
        kind: eml23::FacetKind::Qualifier,
    });
    let from = eml23::ColumnBasedTable {
        uuid: A.to_string(),
        citation: citation("a table of links"),
        column: vec![linking],
        ..Default::default()
    };
    let to = eml23::ColumnBasedTable {
        uuid: B.to_string(),
        citation: citation("the indexes"),
        key_column: vec![text_column("index_uid", &["i1"])],
        ..Default::default()
    };
    let package = Package(vec![(A.to_string(), boxed(from)), (B.to_string(), boxed(to))]);
    let counting = Counting::new(&package);
    let pairs: Vec<(&str, &dyn Reflect)> = package
        .0
        .iter()
        .map(|(uuid, object)| (uuid.as_str(), object.as_reflect()))
        .collect();
    let findings = integrity::run(&pairs, &counting, Some(&counting));
    assert!(
        findings
            .iter()
            .all(|finding| finding.rule != "house/table/link-resolves"
                && finding.rule != "house/table/link-target-key-unique"),
        "the twin links resolve: {findings:?}"
    );
    assert_eq!(
        counting.reads_of(B),
        1,
        "the target table is read once for both link checks"
    );
}

const PROPERTY: &str = "00000000-0000-0000-0000-00000000c301";
const MAP: &str = "00000000-0000-0000-0000-00000000c302";
const OTHER_PROPERTY: &str = "00000000-0000-0000-0000-00000000c303";

/// One colour map decorating several properties is read and validated once by a cascade.
///
/// Break it: in `Cascade::color_information_on`, skip `self.maps` and read the object each time; the map is then
/// read once per property and this fails.
#[test]
fn a_colour_map_shared_by_properties_is_read_once_by_a_cascade() {
    let map = resqml22::ContinuousColorMap {
        uuid: MAP.to_string(),
        citation: citation("map"),
        entry: [0.0, 1.0]
            .iter()
            .map(|index| resqml22::ContinuousColorMapEntry {
                index: *index,
                hsv: resqml22::HsvColor {
                    hue: 120.0,
                    saturation: 1.0,
                    value: 1.0,
                    alpha: 1.0,
                    ..Default::default()
                },
            })
            .collect(),
        ..Default::default()
    };
    let package = Package(vec![(MAP.to_string(), boxed(map))]);
    let counting = Counting::new(&package);
    let set = GraphicalSet {
        entries: [PROPERTY, OTHER_PROPERTY]
            .iter()
            .enumerate()
            .map(|(position, property)| Entry {
                position,
                targets: vec![property.to_string()],
                target_uris: Vec::new(),
                map_uri: None,
                information: Information::Color {
                    scaling: Scaling::default(),
                    map: Some(MAP.to_string()),
                    value_vector_index: None,
                },
            })
            .collect(),
        notes: Vec::new(),
    };
    let cascade = Cascade::new(&set);
    for property in [PROPERTY, OTHER_PROPERTY, PROPERTY] {
        let decision = cascade
            .resolve(
                &Request {
                    representation: "00000000-0000-0000-0000-00000000c3ff",
                    property: Some(property),
                    kind_chain: &[],
                    element: Scope::WholeObject,
                },
                &counting,
            )
            .expect("the cascade runs")
            .expect("the property is decorated");
        assert!(matches!(
            decision.paint,
            energyml_domain::color::Paint::Mapped { .. }
        ));
    }
    assert_eq!(counting.reads_of(MAP), 1, "three questions, one read of the map");
}
