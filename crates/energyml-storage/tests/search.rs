//! `search::find` and `search::resolve_uuid`, over a store made of a `Vec`.
//!
//! The rules under test are the ones the module documents: the criteria
//! combine with AND; `types` is exact-or-`family.*` and case sensitive while
//! `type_contains` and `title` are case-insensitive substrings; an empty answer
//! comes only from a store that listed, while a store that cannot list the
//! dataspace asked for refuses; a bare uuid becomes the full URI, or a named
//! refusal.
//!
//! To make them fail: lowercase the `types` comparison, drop the `AND`, turn
//! the two-dataspace refusal into an empty list, or let `resolve_uuid` pick the
//! first of two candidates.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_storage::array::{ArrayData, ArrayLocation, ArrayMetadata};
use energyml_storage::error::{Diagnostic, Error, Result};
use energyml_storage::resource::{Dataspace, ObjectHeader, ObjectSummary};
use energyml_storage::search::{Query, find, find_report, resolve_uuid};
use energyml_storage::storage::EnergymlStorage;

const U1: &str = "7f000000-0000-4000-8000-000000000001";
const U2: &str = "7f000000-0000-4000-8000-000000000002";
const U3: &str = "7f000000-0000-4000-8000-000000000003";
const U4: &str = "7f000000-0000-4000-8000-000000000004";

fn summary(qualified: &str, uuid: &str, version: Option<&str>) -> ObjectSummary {
    let uri = match version {
        Some(version) => format!("eml:///{qualified}({uuid},version='{version}')"),
        None => format!("eml:///{qualified}({uuid})"),
    };
    ObjectSummary::new(uri, uuid, qualified)
}

/// A store that lists what it was given and reads titles from a side table.
struct Fake {
    dataspaces: Vec<Dataspace>,
    objects: Vec<(ObjectSummary, Option<&'static str>)>,
    /// The uri whose header cannot be read.
    broken: Option<String>,
}

impl Fake {
    fn new(objects: Vec<(ObjectSummary, Option<&'static str>)>) -> Self {
        Self {
            dataspaces: vec![Dataspace::new("eml:///", "fake")],
            objects,
            broken: None,
        }
    }
}

#[async_trait::async_trait]
impl EnergymlStorage for Fake {
    fn store_kind(&self) -> &'static str {
        "fake"
    }

    async fn dataspaces(&self) -> Result<Vec<Dataspace>> {
        Ok(self.dataspaces.clone())
    }

    async fn resources(&self) -> Result<Vec<ObjectSummary>> {
        Ok(self.objects.iter().map(|(summary, _)| summary.clone()).collect())
    }

    async fn headers(&self, uris: &[String]) -> Vec<Result<ObjectHeader>> {
        uris.iter()
            .map(|wanted| {
                if self.broken.as_ref() == Some(wanted) {
                    return Err(Error::Xml("the citation is cut short".into()));
                }
                let (summary, title) = self
                    .objects
                    .iter()
                    .find(|(summary, _)| &summary.uri == wanted)
                    .ok_or_else(|| Error::NotFound(wanted.clone()))?;
                let header = ObjectHeader::new(summary.clone());
                Ok(match title {
                    Some(title) => header.with_title(*title),
                    None => header,
                })
            })
            .collect()
    }

    async fn get_objects(&self, uris: &[String]) -> Vec<Result<Box<dyn DynObject>>> {
        uris.iter()
            .map(|_| {
                Err(Error::Unsupported {
                    operation: "handing over objects",
                    store: "fake",
                })
            })
            .collect()
    }

    async fn supported_types(&self) -> Vec<&'static energyml_core::reflect::TypeInfo> {
        Vec::new()
    }

    fn arrays_of(&self, _object: &dyn Reflect) -> Vec<ArrayLocation> {
        Vec::new()
    }

    async fn array_metadata(&self, _location: &ArrayLocation) -> Result<ArrayMetadata> {
        Err(Error::Unsupported {
            operation: "array metadata",
            store: "fake",
        })
    }

    async fn read_array(&self, _location: &ArrayLocation) -> Result<ArrayData> {
        Err(Error::Unsupported {
            operation: "reading arrays",
            store: "fake",
        })
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

fn block<F: std::future::Future>(future: F) -> F::Output {
    futures_executor::block_on(future)
}

fn store() -> Fake {
    Fake::new(vec![
        (
            summary("resqml22.TriangulatedSetRepresentation", U1, None),
            Some("Top Brent"),
        ),
        (
            summary("resqml22.Grid2dRepresentation", U2, None),
            Some("Base Brent grid"),
        ),
        (
            summary("resqml20.obj_Grid2dRepresentation", U3, None),
            Some("Legacy top"),
        ),
        (summary("eml23.LocalEngineeringCompoundCrs", U4, None), None),
    ])
}

fn uuids(found: &[ObjectSummary]) -> Vec<&str> {
    found.iter().map(|s| s.uuid.as_str()).collect()
}

#[test]
fn an_empty_query_lists_the_whole_store_in_the_stores_order() {
    let found = block(find(&store(), &Query::new())).unwrap();
    assert_eq!(uuids(&found), vec![U1, U2, U3, U4]);
}

#[test]
fn a_dataspace_and_an_exact_type() {
    let query = Query::new()
        .in_dataspace("eml:///")
        .with_types(["resqml22.Grid2dRepresentation"]);
    let found = block(find(&store(), &query)).unwrap();
    assert_eq!(uuids(&found), vec![U2]);

    // Negative: the same type with another case is another type (ETP type
    // names are case sensitive), and that is an empty answer from a store
    // that listed.
    let query = Query::new()
        .in_dataspace("eml:///")
        .with_types(["resqml22.grid2drepresentation"]);
    assert!(block(find(&store(), &query)).unwrap().is_empty());
}

#[test]
fn a_family_wildcard_keeps_one_schema_version() {
    for query in [
        Query::new().with_types(["resqml22.*"]),
        Query::new().in_dataspace("eml:///").with_types(["resqml22.*"]),
    ] {
        let found = block(find(&store(), &query)).unwrap();
        assert_eq!(uuids(&found), vec![U1, U2], "{query:?}");
    }
    // Negative: the wildcard is the version, not a prefix of it.
    let found = block(find(&store(), &Query::new().with_types(["resqml2.*"]))).unwrap();
    assert!(found.is_empty());
}

#[test]
fn a_malformed_wildcard_is_refused_with_or_without_a_dataspace() {
    for query in [
        Query::new().with_types(["resqml22.*x"]),
        Query::new().in_dataspace("eml:///").with_types(["resqml22.*x"]),
        Query::new().with_types(["*"]),
    ] {
        let error = block(find(&store(), &query)).unwrap_err();
        assert!(matches!(error, Error::BadUri { .. }), "{query:?}: {error}");
    }
}

#[test]
fn a_type_substring_ignores_the_case() {
    let found = block(find(&store(), &Query::new().type_containing("GRID2D"))).unwrap();
    assert_eq!(uuids(&found), vec![U2, U3]);
    // Negative: `types` is not a substring.
    let found = block(find(&store(), &Query::new().with_types(["Grid2d"]))).unwrap();
    assert!(found.is_empty());
}

#[test]
fn a_title_substring_ignores_the_case_and_reads_the_headers() {
    let found = block(find(&store(), &Query::new().titled("BRENT"))).unwrap();
    assert_eq!(uuids(&found), vec![U1, U2]);

    let report = block(find_report(&store(), &Query::new().titled("brent"))).unwrap();
    assert_eq!(report.listed, 4);
    assert_eq!(report.headers_read, 4, "every candidate's header was read");

    // Negative: an object without a title is not found by one, and a title
    // that is nowhere gives the real empty answer.
    let found = block(find(&store(), &Query::new().titled("zzz"))).unwrap();
    assert!(found.is_empty());
}

#[test]
fn criteria_combine_with_and_and_the_title_pass_reads_only_what_is_left() {
    let query = Query::new().with_types(["resqml22.*"]).titled("top");
    let found = block(find(&store(), &query)).unwrap();
    assert_eq!(
        uuids(&found),
        vec![U1],
        "`Legacy top` is resqml20, `Base Brent grid` has no `top`"
    );

    let query = Query::new()
        .with_types(["resqml22.*"])
        .type_containing("grid")
        .titled("brent");
    let report = block(find_report(&store(), &query)).unwrap();
    assert_eq!(uuids(&report.objects), vec![U2]);
    assert_eq!(report.listed, 2, "the type filter came first");
    assert_eq!(report.headers_read, 1, "and only the survivor's header was read");

    // Negative: an AND that became an OR would return the three of the first
    // criterion's union with the second.
    let query = Query::new().type_containing("Crs").titled("brent");
    assert!(block(find(&store(), &query)).unwrap().is_empty());
}

#[test]
fn a_store_that_cannot_list_the_dataspace_refuses_instead_of_answering_none() {
    let mut two = store();
    two.dataspaces.push(Dataspace::new("eml:///dataspace('b')", "b"));
    // The same store, asked for its whole listing, answers.
    assert_eq!(block(find(&two, &Query::new())).unwrap().len(), 4);
    // Asked for a dataspace, it cannot say which one the listing belongs to.
    let error = block(find(&two, &Query::new().in_dataspace("eml:///"))).unwrap_err();
    assert!(matches!(error, Error::Unsupported { .. }), "{error}");

    // A dataspace the store does not hold is not an empty dataspace.
    let error = block(find(
        &store(),
        &Query::new().in_dataspace("eml:///dataspace('other')"),
    ))
    .unwrap_err();
    assert!(matches!(error, Error::NotFound(_)), "{error}");
}

#[test]
fn an_unreadable_header_refuses_the_search_and_the_report_keeps_it_aside() {
    let mut broken = store();
    broken.broken = Some(summary("resqml22.Grid2dRepresentation", U2, None).uri);

    let error = block(find(&broken, &Query::new().titled("brent"))).unwrap_err();
    assert!(matches!(error, Error::Xml(_)), "{error}");

    let report = block(find_report(&broken, &Query::new().titled("brent"))).unwrap();
    assert_eq!(uuids(&report.objects), vec![U1]);
    assert_eq!(report.unreadable.len(), 1);

    // Negative: without a title no header is read, so the broken one is not
    // in the way.
    let found = block(find(&broken, &Query::new().type_containing("grid"))).unwrap();
    assert_eq!(found.len(), 2);
}

fn versioned() -> Fake {
    Fake::new(vec![
        (
            summary("resqml22.Grid2dRepresentation", U1, Some("1")),
            Some("v1"),
        ),
        (
            summary("resqml22.Grid2dRepresentation", U1, Some("2")),
            Some("v2"),
        ),
        // The same uuid under another type.
        (
            summary("resqml22.PointSetRepresentation", U1, None),
            Some("points"),
        ),
        (summary("resqml22.Grid2dRepresentation", U2, None), Some("other")),
    ])
}

#[test]
fn a_bare_uuid_becomes_the_full_uri() {
    let uri = block(resolve_uuid(&store(), U2, None)).unwrap();
    assert_eq!(uri, summary("resqml22.Grid2dRepresentation", U2, None).uri);

    // The case of the uuid does not matter, the way `EpcStore::lookup_uuid` has it.
    let upper = U2.to_ascii_uppercase();
    assert_eq!(block(resolve_uuid(&store(), &upper, None)).unwrap(), uri);

    // And the type, when given, is checked.
    assert_eq!(
        block(resolve_uuid(&store(), U2, Some("resqml22.Grid2dRepresentation"))).unwrap(),
        uri
    );
}

#[test]
fn a_uuid_nobody_holds_is_not_found_by_name() {
    let missing = "7f000000-0000-4000-8000-0000000000ff";
    match block(resolve_uuid(&store(), missing, None)).unwrap_err() {
        Error::NotFound(named) => assert_eq!(named, missing),
        other => panic!("expected NotFound, got {other}"),
    }
    // The right uuid with the wrong type is not found either, and says which.
    match block(resolve_uuid(
        &store(),
        U2,
        Some("resqml22.PointSetRepresentation"),
    ))
    .unwrap_err()
    {
        Error::NotFound(named) => assert_eq!(named, format!("resqml22.PointSetRepresentation({U2})")),
        other => panic!("expected NotFound, got {other}"),
    }
    // Braces are not stripped: the lookup it follows does not strip them.
    let braced = format!("{{{U2}}}");
    assert!(matches!(
        block(resolve_uuid(&store(), &braced, None)).unwrap_err(),
        Error::NotFound(_)
    ));
    assert!(matches!(
        block(resolve_uuid(&store(), "", None)).unwrap_err(),
        Error::BadUri { .. }
    ));
}

#[test]
fn a_uuid_held_twice_is_ambiguous_and_names_every_candidate() {
    // Two versions and a second type: three objects carry U1.
    match block(resolve_uuid(&versioned(), U1, None)).unwrap_err() {
        Error::Ambiguous { uri, candidates } => {
            assert_eq!(uri, U1);
            assert_eq!(candidates.len(), 3, "{candidates:?}");
            assert!(candidates.iter().any(|c| c.contains("version='1'")));
            assert!(candidates.iter().any(|c| c.contains("version='2'")));
            assert!(candidates.iter().any(|c| c.contains("PointSetRepresentation")));
        }
        other => panic!("expected Ambiguous, got {other}"),
    }
    // Narrowed by type, two versions are left: still ambiguous, and the
    // candidates are only those of that type.
    match block(resolve_uuid(
        &versioned(),
        U1,
        Some("resqml22.Grid2dRepresentation"),
    ))
    .unwrap_err()
    {
        Error::Ambiguous { candidates, .. } => assert_eq!(candidates.len(), 2),
        other => panic!("expected Ambiguous, got {other}"),
    }
}

#[test]
fn the_type_narrows_a_uuid_held_under_two_types() {
    let uri = block(resolve_uuid(
        &versioned(),
        U1,
        Some("resqml22.PointSetRepresentation"),
    ))
    .unwrap();
    assert_eq!(uri, summary("resqml22.PointSetRepresentation", U1, None).uri);
}
