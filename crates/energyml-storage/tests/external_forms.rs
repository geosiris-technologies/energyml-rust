//! The three shapes an EnergyML object uses to say *where the numbers are*.
//!
//! There are three, not two, and the middle one was missing until 31/08:
//!
//! | eml | type | how it names the file | how it names the dataset |
//! |---|---|---|---|
//! | 2.0, 2.1 | `Hdf5Dataset` | a DOR, `HdfProxy` | `PathInHdfFile` |
//! | 2.1, 2.2 | `ExternalDatasetPart` | a DOR, `EpcExternalPartReference` | `PathInExternalFile` |
//! | 2.3 | `ExternalDataArrayPart` | `URI`, on the part itself | `PathInExternalFile` |
//!
//! The middle one is neither of its neighbours — the older shape's proxy
//! reference with the newer shape's path — and nothing matched it, so every
//! array of every eml 2.1/2.2 package came back as *"declares no external data
//! at all"*. That is witsml 2.0, prodml 2.0 and resqml 2.2_dev3.
//!
//! Found on a 1344-object client package whose 362 polylines refused to draw
//! with their coordinates sitting in the document. After: 390 representations
//! evaluated, 1 938 468 points.

use energyml_storage::array::{ProxyTarget, locate_with};

/// The proxy a package would resolve, stubbed: one uuid, one file.
fn resolver(uuid: &str) -> Option<ProxyTarget> {
    (uuid == "c2276841-4e15-4db1-83c9-10b8e78749a9").then(|| ProxyTarget {
        file: "model.h5".to_string(),
        mime: Some("application/x-hdf5".to_string()),
    })
}

/// eml 2.2's `ExternalDatasetPart` is found, and resolves through its proxy.
#[test]
fn the_eml_2_2_form_names_a_file_and_a_dataset() {
    use energyml_eml_2_2::{DataObjectReference, ExternalDataset, ExternalDatasetPart};

    let dataset = ExternalDataset {
        external_file_proxy: vec![ExternalDatasetPart {
            count: 1,
            start_index: 0,
            path_in_external_file: "/RESQML/abc/pnt_patch.Line".to_string(),
            epc_external_part_reference: DataObjectReference {
                uuid: "c2276841-4e15-4db1-83c9-10b8e78749a9".to_string(),
                title: "External HDF5 proxy".to_string(),
                ..Default::default()
            },
        }],
    };

    let found = locate_with(&dataset, &resolver);
    assert_eq!(found.len(), 1, "the part is a location: {found:?}");
    let location = &found[0];
    assert_eq!(location.file, "model.h5", "resolved through the proxy");
    assert_eq!(location.path_in_file, "/RESQML/abc/pnt_patch.Line");
    assert_eq!(location.mime.as_deref(), Some("application/x-hdf5"));
    assert_eq!(
        location.proxy_uuid.as_deref(),
        Some("c2276841-4e15-4db1-83c9-10b8e78749a9"),
        "the proxy is named, so a caller can say which one was followed"
    );
}

/// **`StartIndex` and `Count` are not a hyperslab, and are not read as one.**
///
/// They are single values describing a *flat* range — `ExternalFileProxy`
/// repeats, so an array may be split across parts — while `start`/`count` on a
/// location are the per-dimension selection eml 2.3 introduced. Passing one as
/// the other is not a small error: it made every polyline of the client package
/// ask for **one** dimension of a **two**-dimensional dataset and refuse, with
/// nothing wrong in the file.
///
/// Empty means "the whole dataset", which is what the eml 2.0/2.1 form has
/// always done and what a single part means anyway.
#[test]
fn the_flat_range_is_not_mistaken_for_a_per_dimension_selection() {
    use energyml_eml_2_2::{DataObjectReference, ExternalDataset, ExternalDatasetPart};

    let dataset = ExternalDataset {
        external_file_proxy: vec![ExternalDatasetPart {
            // Deliberately not 0 and not 1: if either ever reaches the
            // selection, this fails loudly instead of by coincidence.
            count: 7,
            start_index: 3,
            path_in_external_file: "/points".to_string(),
            epc_external_part_reference: DataObjectReference {
                uuid: "c2276841-4e15-4db1-83c9-10b8e78749a9".to_string(),
                ..Default::default()
            },
        }],
    };

    let found = locate_with(&dataset, &resolver);
    assert!(
        found[0].start.is_empty() && found[0].count.is_empty(),
        "a flat range must not become a hyperslab: {:?} / {:?}",
        found[0].start,
        found[0].count
    );
}

/// An unresolvable proxy yields a location with no file, not no location.
///
/// The distinction is the one this crate keeps everywhere: "we could not find
/// where it lives" and "it does not point anywhere" are different answers, and
/// a caller that saw an empty list could not tell them apart.
#[test]
fn a_proxy_nobody_knows_still_yields_the_dataset_path() {
    use energyml_eml_2_2::{DataObjectReference, ExternalDataset, ExternalDatasetPart};

    let dataset = ExternalDataset {
        external_file_proxy: vec![ExternalDatasetPart {
            count: 1,
            start_index: 0,
            path_in_external_file: "/points".to_string(),
            epc_external_part_reference: DataObjectReference {
                uuid: "00000000-0000-4000-8000-000000000000".to_string(),
                ..Default::default()
            },
        }],
    };

    let found = locate_with(&dataset, &resolver);
    assert_eq!(found.len(), 1);
    assert!(found[0].file.is_empty(), "no file was resolved");
    assert_eq!(found[0].path_in_file, "/points", "but the path is still known");
}
