//! `energyml check` and the property counts (04/10/2026): `Property::check` was
//! called by `energyml properties --check` only, so a property holding 7 values
//! on a 12-node grid went unreported by the command that says a package is
//! sound.
//!
//! The rule is stated in RESQML TRG 2.2 §7.1 and Properties.xsd
//! (`ValueCountPerIndexableElement`): a property holds `elements × values per
//! element` values. The grid and the arrays are built here, inline: no file.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::check::{self, Options, Package, Part};
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata};
use energyml_storage::resource::ObjectSummary;

const GRID: &str = "00000000-0000-0000-0000-0000000000b1";
const PROPERTY: &str = "00000000-0000-0000-0000-0000000000b2";

struct Held(Vec<(String, Box<dyn DynObject>)>);

impl ObjectSource for Held {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(uuid))
            .map(|(_, one)| one.clone_box())
    }
}

impl ArraySource for Held {
    fn locate(&self, _object: &dyn Reflect) -> Vec<ArrayLocation> {
        Vec::new()
    }
    fn metadata(&self, _location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported(
            "an external array",
            "the test's arrays are inline",
        ))
    }
    fn read(&self, _location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        Err(energyml_domain::Error::unsupported(
            "an external array",
            "the test's arrays are inline",
        ))
    }
    fn read_into(
        &self,
        _location: &ArrayLocation,
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported(
            "an external array",
            "the test's arrays are inline",
        ))
    }
}

fn summary(uuid: &str, ty: &str) -> ObjectSummary {
    let name = ty.split('.').nth(1).unwrap();
    ObjectSummary::new(format!("eml:///{ty}({uuid})"), uuid.to_string(), ty.to_string())
        .with_content_type(format!("application/x-resqml+xml;version=2.2;type={name}"))
        .with_locator(format!("{name}_{uuid}.xml"))
}

fn citation() -> eml23::Citation {
    eml23::Citation {
        title: "t".into(),
        originator: "o".into(),
        creation: "2026-10-04T00:00:00Z".into(),
        format: "f".into(),
        ..Default::default()
    }
}

/// A grid of 3 × 4 = 12 nodes (Grids.xsd, `Grid2dRepresentation`).
fn grid() -> resqml22::Grid2dRepresentation {
    resqml22::Grid2dRepresentation {
        uuid: GRID.into(),
        citation: citation(),
        slowest_axis_count: 3,
        fastest_axis_count: 4,
        ..Default::default()
    }
}

/// A property on the nodes of that grid, holding `values` values.
fn property(values: i64) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: PROPERTY.into(),
        citation: citation(),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![1],
        supporting_representation: eml23::DataObjectReference {
            uuid: GRID.into(),
            qualified_type: "resqml22.Grid2dRepresentation".into(),
            title: "g".into(),
            ..Default::default()
        },
        values_for_patch: vec![resqml22::AbstractValueArray::FloatingPointConstantArray(
            eml23::FloatingPointConstantArray {
                value: 1.0,
                count: values,
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

fn judge(values: i64) -> check::Report {
    let held = Held(vec![
        (GRID.to_string(), Box::new(grid()) as Box<dyn DynObject>),
        (
            PROPERTY.to_string(),
            Box::new(property(values)) as Box<dyn DynObject>,
        ),
    ]);
    let summaries = vec![
        summary(GRID, "resqml22.Grid2dRepresentation"),
        summary(PROPERTY, "resqml22.ContinuousProperty"),
    ];
    let parts: Vec<Part<'_>> = summaries
        .iter()
        .zip(&held.0)
        .map(|(summary, (_, object))| Part {
            summary,
            object: object.as_ref(),
            relationships: &[],
        })
        .collect();
    let registry = energyml_schemas::all();
    let package = Package {
        objects: &parts,
        summaries: &summaries,
        unreadable: &[],
        diagnostics: &[],
        source: &held,
        known: Some(&registry),
    };
    check::check_with_arrays(
        &package,
        &Options {
            schema: false,
            rules: true,
            extra_rules: Vec::new(),
        },
        Some(&held),
    )
}

#[test]
fn the_right_count_is_silent() {
    let report = judge(12);
    assert!(report.counts.is_empty(), "{:?}", report.counts);
    assert_eq!(
        report.counts_unchecked, 0,
        "the grid counts its nodes, so it was judged"
    );
}

#[test]
fn a_count_that_does_not_fit_is_an_error_of_the_file() {
    let report = judge(7);
    assert_eq!(report.counts.len(), 1, "{:?}", report.counts);
    assert!(
        report.counts[0].is_error
            && report.counts[0].says.contains("12")
            && report.counts[0].says.contains('7'),
        "{:?}",
        report.counts
    );
    assert_eq!(report.errors(), 1, "and it counts");
}

/// 24 = 2 × 12: a time series in one dataset, which the guides neither describe
/// nor forbid. Said, listed, not condemned.
#[test]
fn a_whole_multiple_is_a_remark_not_an_error() {
    let report = judge(24);
    assert_eq!(report.counts.len(), 1, "{:?}", report.counts);
    assert!(!report.counts[0].is_error, "{:?}", report.counts);
    assert_eq!(report.errors(), 0);
    assert_eq!(
        report.warnings(),
        judge(12).warnings(),
        "listed and not counted: no more warnings than the same package with the right count"
    );
}

/// Without something to count the values with, the section is **not run**, and
/// says so - never a clean zero.
#[test]
fn with_no_array_source_the_counts_are_unchecked_not_fine() {
    let held = Held(vec![
        (GRID.to_string(), Box::new(grid()) as Box<dyn DynObject>),
        (PROPERTY.to_string(), Box::new(property(7)) as Box<dyn DynObject>),
    ]);
    let summaries = vec![
        summary(GRID, "resqml22.Grid2dRepresentation"),
        summary(PROPERTY, "resqml22.ContinuousProperty"),
    ];
    let parts: Vec<Part<'_>> = summaries
        .iter()
        .zip(&held.0)
        .map(|(summary, (_, object))| Part {
            summary,
            object: object.as_ref(),
            relationships: &[],
        })
        .collect();
    let package = Package {
        objects: &parts,
        summaries: &summaries,
        unreadable: &[],
        diagnostics: &[],
        source: &held,
        known: None,
    };
    let report = check::check(
        &package,
        &Options {
            schema: false,
            rules: true,
            extra_rules: Vec::new(),
        },
    );
    assert!(report.counts.is_empty());
    assert_eq!(report.counts_unchecked, 1);
}
