//! The readers, driven from a store that is not an EPC.
//!
//! `energyml-domain` was written against [`ObjectSource`] and [`ArraySource`]
//! rather than against `EpcStore`, so that an ETP client could be dropped in.
//! Only one thing supplied those traits, though - an EPC - so the claim rested
//! on a seam nothing had ever crossed.
//!
//! The seam has a real obstacle behind it, and it is not the trait shape: the
//! access traits are **synchronous** and `EnergymlStorage` is **asynchronous**,
//! because ETP is a WebSocket and a browser cannot block on a future. `EpcStore`
//! gets across by having a synchronous API of its own. A network client has
//! none, so the crossing is [`Prefetched`]: one asynchronous pass that pulls
//! what is needed, then synchronous reads out of it - which is the shape ETP
//! was designed for, every request being plural.
//!
//! What is checked here:
//!
//! - a CRS reads from a prefetch, with no EPC anywhere;
//! - the transitive follow really follows (depth 0 does not, depth 1 does);
//! - an array that was not prefetched is refused **by name**, not returned
//!   empty - a geometry reader that silently sees no points draws nothing and
//!   says nothing, which is the failure this rules out.

use std::sync::Arc;

use energyml_core::registry::{DynObject, Registry};
use energyml_core::value::Reflect;
use energyml_domain::source::{ArraySource, ObjectSource, Prefetched};
use energyml_storage::array::{ArrayData, ArrayLocation, ArrayMetadata, ArrayValues, NoProxies, locate_with};
use energyml_storage::error::{Diagnostic, Error, Result};
use energyml_storage::resource::{Dataspace, ObjectHeader, ObjectSummary};
use energyml_storage::storage::EnergymlStorage;

const CRS: &str = "aa000000-0000-4000-8000-000000000001";
const REPRESENTATION: &str = "bb000000-0000-4000-8000-000000000002";

fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_0::register(&mut registry);
    energyml_resqml_2_0_1::register(&mut registry);
    Arc::new(registry)
}

fn uri(qualified_type: &str, uuid: &str) -> String {
    format!("eml:///{qualified_type}({uuid})")
}

fn citation(title: &str) -> String {
    format!(
        "<eml:Citation><eml:Title>{title}</eml:Title>\
<eml:Originator>test</eml:Originator>\
<eml:Creation>2026-08-02T00:00:00Z</eml:Creation>\
<eml:Format>test</eml:Format></eml:Citation>"
    )
}

/// A CRS, and a point set that names it and points at an array.
///
/// The point set is the object a geometry reader would be handed; the CRS is
/// the one it has to *follow a reference* to reach. Two hops is the whole
/// question the prefetch has to answer.
fn documents() -> Vec<(ObjectSummary, String)> {
    let crs = format!(
        "<resqml:obj_LocalDepth3dCrs \
xmlns:resqml=\"http://www.energistics.org/energyml/data/resqmlv2\" \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
uuid=\"{CRS}\" schemaVersion=\"2.0\">\
{}\
<resqml:XOffset>1000</resqml:XOffset>\
<resqml:YOffset>2000</resqml:YOffset>\
<resqml:ZOffset>0</resqml:ZOffset>\
<resqml:ArealRotation uom=\"rad\">0</resqml:ArealRotation>\
<resqml:ProjectedAxisOrder>easting northing</resqml:ProjectedAxisOrder>\
<resqml:ProjectedUom>m</resqml:ProjectedUom>\
<resqml:VerticalUom>m</resqml:VerticalUom>\
<resqml:ZIncreasingDownward>true</resqml:ZIncreasingDownward>\
<resqml:VerticalCrs xsi:type=\"eml:VerticalUnknownCrs\" \
xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
<eml:Unknown>none</eml:Unknown></resqml:VerticalCrs>\
<resqml:ProjectedCrs xsi:type=\"eml:ProjectedCrsEpsgCode\" \
xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
<eml:EpsgCode>32631</eml:EpsgCode></resqml:ProjectedCrs>\
</resqml:obj_LocalDepth3dCrs>",
        citation("a local depth CRS")
    );

    let representation = format!(
        "<resqml:obj_PointSetRepresentation \
xmlns:resqml=\"http://www.energistics.org/energyml/data/resqmlv2\" \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
uuid=\"{REPRESENTATION}\" schemaVersion=\"2.0\">\
{}\
<resqml:NodePatch><resqml:PatchIndex>0</resqml:PatchIndex>\
<resqml:Count>2</resqml:Count>\
<resqml:Geometry>\
<resqml:LocalCrs>\
<eml:ContentType>application/x-resqml+xml;version=2.0;type=obj_LocalDepth3dCrs</eml:ContentType>\
<eml:Title>a local depth CRS</eml:Title><eml:UUID>{CRS}</eml:UUID>\
</resqml:LocalCrs>\
<resqml:Points xsi:type=\"resqml:Point3dHdf5Array\" \
xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
<resqml:Coordinates><eml:PathInHdfFile>/points</eml:PathInHdfFile>\
<eml:HdfProxy><eml:ContentType>application/x-eml+xml;version=2.0;type=obj_EpcExternalPartReference</eml:ContentType>\
<eml:Title>proxy</eml:Title><eml:UUID>cc000000-0000-4000-8000-000000000003</eml:UUID>\
</eml:HdfProxy></resqml:Coordinates>\
</resqml:Points>\
</resqml:Geometry></resqml:NodePatch>\
</resqml:obj_PointSetRepresentation>",
        citation("a point set")
    );

    let summary = |ty: &str, uuid: &str, name: &str, title: &str| {
        let _ = title;
        ObjectSummary {
            uri: uri(ty, uuid),
            uuid: uuid.to_string(),
            qualified_type: ty.to_string(),
            content_type: format!("application/x-resqml+xml;version=2.0;type={name}"),
            locator: format!("in-memory:{uuid}"),
        }
    };
    vec![
        (
            summary("resqml20.obj_LocalDepth3dCrs", CRS, "obj_LocalDepth3dCrs", "crs"),
            crs,
        ),
        (
            summary(
                "resqml20.obj_PointSetRepresentation",
                REPRESENTATION,
                "obj_PointSetRepresentation",
                "points",
            ),
            representation,
        ),
    ]
}

/// A store with no file behind it, standing in for the ETP client.
struct Remote {
    registry: Arc<Registry>,
    parts: Vec<(ObjectSummary, String)>,
}

impl Remote {
    fn new() -> Self {
        Remote {
            registry: registry(),
            parts: documents(),
        }
    }
}

#[async_trait::async_trait]
impl EnergymlStorage for Remote {
    fn store_kind(&self) -> &'static str {
        "remote"
    }

    async fn dataspaces(&self) -> Result<Vec<Dataspace>> {
        Ok(vec![Dataspace {
            uri: "eml:///".to_string(),
            name: "remote".to_string(),
            created: None,
            last_changed: None,
        }])
    }

    async fn resources(&self) -> Result<Vec<ObjectSummary>> {
        Ok(self.parts.iter().map(|(s, _)| s.clone()).collect())
    }

    async fn headers(&self, uris: &[String]) -> Vec<Result<ObjectHeader>> {
        uris.iter()
            .map(|wanted| {
                let (summary, _) = self
                    .parts
                    .iter()
                    .find(|(s, _)| &s.uri == wanted)
                    .ok_or_else(|| Error::NotFound(wanted.clone()))?;
                Ok(ObjectHeader {
                    summary: summary.clone(),
                    title: None,
                    originator: None,
                    creation: None,
                    last_update: None,
                    format: None,
                    object_version: None,
                    schema_version: Some("2.0".to_string()),
                })
            })
            .collect()
    }

    async fn get_objects(&self, uris: &[String]) -> Vec<Result<Box<dyn DynObject>>> {
        uris.iter()
            .map(|wanted| {
                let (summary, source) = self
                    .parts
                    .iter()
                    .find(|(s, _)| &s.uri == wanted)
                    .ok_or_else(|| Error::NotFound(wanted.clone()))?;
                let entry = self
                    .registry
                    .by_qualified_type(&summary.qualified_type)
                    .ok_or_else(|| Error::UnknownType(summary.qualified_type.clone()))?;
                let content_type = entry
                    .info
                    .content_type
                    .ok_or_else(|| Error::UnknownType(summary.qualified_type.clone()))?;
                Ok(self.registry.read_xml(content_type, source.as_bytes())?)
            })
            .collect()
    }

    async fn supported_types(&self) -> Vec<&'static energyml_core::reflect::TypeInfo> {
        self.registry.types().collect()
    }

    fn arrays_of(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        // A dataspace has no `EpcExternalPartReference` to resolve: the array
        // is addressed by the object's URI and a path. The proxy is left
        // unresolved and the path is what the store reads by - which is
        // exactly the open shape question about
        // `ArrayLocation::file`.
        locate_with(object, &NoProxies)
    }

    async fn array_metadata(&self, location: &ArrayLocation) -> Result<ArrayMetadata> {
        Ok(self.read_array(location).await?.metadata)
    }

    async fn read_array(&self, location: &ArrayLocation) -> Result<ArrayData> {
        if location.path_in_file != "/points" {
            return Err(Error::NotFound(location.path_in_file.clone()));
        }
        Ok(ArrayData {
            metadata: ArrayMetadata {
                dimensions: vec![2, 3],
                element_type: "double".to_string(),
            },
            values: ArrayValues::F64(vec![0.0, 0.0, 0.0, 10.0, 20.0, 30.0]),
        })
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

fn block<F: std::future::Future>(future: F) -> F::Output {
    futures_executor::block_on(future)
}

#[test]
fn a_prefetch_of_depth_one_reaches_the_crs_the_representation_names() {
    let store = Remote::new();
    let roots = vec![uri("resqml20.obj_PointSetRepresentation", REPRESENTATION)];

    let shallow = block(Prefetched::fetch(&store, &roots, 0)).expect("depth 0");
    assert_eq!(shallow.len(), 1, "depth 0 is the roots and nothing else");
    assert!(
        shallow.object(CRS).is_none(),
        "the CRS is a hop away and depth 0 does not take it"
    );

    let deep = block(Prefetched::fetch(&store, &roots, 1)).expect("depth 1");
    assert_eq!(deep.len(), 2);
    assert!(deep.object(CRS).is_some(), "one hop reaches the CRS");
}

#[test]
fn a_crs_reads_out_of_a_prefetch_with_no_epc_anywhere() {
    // The point of the whole seam: `crs::read` takes `&dyn ObjectSource` and
    // has no idea what is behind it.
    let store = Remote::new();
    let roots = vec![uri("resqml20.obj_LocalDepth3dCrs", CRS)];
    let cache = block(Prefetched::fetch(&store, &roots, 0)).expect("a prefetch");

    let object = cache.object(CRS).expect("the CRS is cached");
    let crs = energyml_domain::crs::read_with(object.as_reflect(), &cache).expect("it reads");
    assert_eq!(crs.projected.epsg(), Some(32631));
    // The offsets are what a point placed through this CRS is moved by; if the
    // prefetch had handed back an empty object they would be zero.
    let placed = crs.to_projected([0.0, 0.0, 0.0]);
    assert!(
        (placed[0] - 1000.0).abs() < 1e-9 && (placed[1] - 2000.0).abs() < 1e-9,
        "the offsets came through the prefetch: {placed:?}"
    );
}

#[test]
fn an_array_is_located_by_the_prefetch_and_read_out_of_it() {
    let store = Remote::new();
    let roots = vec![uri("resqml20.obj_PointSetRepresentation", REPRESENTATION)];
    let mut cache = block(Prefetched::fetch(&store, &roots, 1)).expect("a prefetch");

    let located = cache.locations_of(REPRESENTATION);
    assert_eq!(located.len(), 1, "one array: the coordinates");
    assert_eq!(located[0].path_in_file, "/points");
    assert!(
        located[0].field_path.contains("Coordinates"),
        "the location says where in the object it came from: {}",
        located[0].field_path
    );

    let wanted = cache.locations();
    assert_eq!(block(cache.fetch_arrays(&store, &wanted)), 1);

    // And now the synchronous side, which is all any reader above ever sees.
    let object = cache.object(REPRESENTATION).expect("it is cached");
    let locations = ArraySource::locate(&cache, object.as_reflect());
    assert_eq!(locations.len(), 1);
    let data = ArraySource::read(&cache, &locations[0]).expect("it reads");
    assert_eq!(data.metadata.dimensions, vec![2, 3]);
    assert_eq!(data.f64().expect("doubles"), &[0.0, 0.0, 0.0, 10.0, 20.0, 30.0]);
}

#[test]
fn an_array_that_was_not_prefetched_is_refused_by_name() {
    // The failure this rules out: a reader that gets an empty answer draws a
    // representation with no points and reports nothing wrong.
    let store = Remote::new();
    let roots = vec![uri("resqml20.obj_PointSetRepresentation", REPRESENTATION)];
    let cache = block(Prefetched::fetch(&store, &roots, 1)).expect("a prefetch");

    let locations = cache.locations();
    assert_eq!(locations.len(), 1);
    let message = match ArraySource::read(&cache, &locations[0]) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("nothing was prefetched, so nothing may be read"),
    };
    assert!(
        message.contains("/points") && message.contains("prefetch"),
        "the refusal has to name the array and say what to do: {message}"
    );
}

#[test]
fn reading_into_a_buffer_refuses_a_type_that_does_not_match() {
    use energyml_storage::array::ArrayBuffer;

    let store = Remote::new();
    let roots = vec![uri("resqml20.obj_PointSetRepresentation", REPRESENTATION)];
    let mut cache = block(Prefetched::fetch(&store, &roots, 1)).expect("a prefetch");
    let locations = cache.locations();
    block(cache.fetch_arrays(&store, &locations));

    // A buffer of the right type takes the values.
    let mut room = vec![0.0f64; 6];
    let metadata = ArraySource::read_into(&cache, &locations[0], &mut ArrayBuffer::F64(&mut room))
        .expect("the doubles go in");
    assert_eq!(metadata.dimensions, vec![2, 3]);
    assert_eq!(room, vec![0.0, 0.0, 0.0, 10.0, 20.0, 30.0]);

    // A buffer of the wrong type does not take the bytes and pretend.
    let mut wrong = vec![0i32; 6];
    let message = match ArraySource::read_into(&cache, &locations[0], &mut ArrayBuffer::I32(&mut wrong)) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("doubles must not be poured into a buffer of int32"),
    };
    assert!(
        message.contains("double") && message.contains("int32"),
        "the refusal names both types: {message}"
    );
    assert!(wrong.iter().all(|value| *value == 0));
}

/// The part of an object a value reader hands over - the `Points` of a node
/// patch, which has no uuid - finds its array in the prefetch. Every reader
/// calls `source.locate(array)` with such a part; until 15/09 the prefetch
/// only answered for whole objects, so a geometry read through it drew
/// nothing. Found by the ETP bridge's acceptance test.
#[test]
fn a_part_of_an_object_finds_its_arrays_in_the_prefetch() {
    use energyml_core::value::FieldValue;

    fn part<'a>(object: &'a dyn Reflect, path: &[&str]) -> &'a dyn Reflect {
        let mut here = object;
        for name in path {
            here = match here.field(name).unwrap_or_else(|| panic!("no `{name}`")) {
                FieldValue::Object(child) => child,
                FieldValue::List(items) => match items.into_iter().next() {
                    Some(FieldValue::Object(child)) => child,
                    other => panic!("`{name}` holds {other:?}"),
                },
                _ => panic!("`{name}` is not an object"),
            };
        }
        here
    }

    let store = Remote::new();
    let roots = vec![uri("resqml20.obj_PointSetRepresentation", REPRESENTATION)];
    let cache = block(Prefetched::fetch(&store, &roots, 1)).expect("a prefetch");
    let object = cache.object(REPRESENTATION).expect("it is cached");
    let points = part(object.as_reflect(), &["node_patch", "geometry", "points"]);
    assert!(
        energyml_core::identity::uuid_of_reflect(points).is_none(),
        "a part, not an object"
    );
    assert_eq!(
        ArraySource::locate(&cache, points),
        cache.locations_of(REPRESENTATION)
    );
}
