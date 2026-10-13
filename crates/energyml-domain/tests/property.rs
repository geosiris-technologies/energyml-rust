//! The property view, against every property of the reference packages.
//!
//! What is checked is not "it parses". It is the three things a property
//! reader can get wrong while looking perfectly healthy:
//!
//! 1. **the same property read from the 2.0.1 and the 2.2 package must come
//!    out identical** — the two reference packages hold the same objects under
//!    the same uuids, written to different schemas, which makes them a
//!    differential test nobody had to write;
//! 2. **the patch attachment**, which is by explicit index in 2.0.1 and by
//!    position in 2.2 — the one place a naive generic reader breaks silently;
//! 3. **the counts**, cross-checked against the shape the HDF5 file actually
//!    holds where the `hdf5` feature is on.
//!
//! `testdata/` is git-ignored, so everything here skips when it is absent.

use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_domain::property::{self, IndexableElement, PropertyKindRef};
use energyml_storage::EpcStore;

/// Every schema version the workspace generates.
///
/// One line rather than a hand-kept subset: a list written out here goes stale
/// the day a release is added, and nothing would notice — the suite would keep
/// passing while quietly never covering the new version. `energyml-schemas`
/// holds the one list, and its own test fails when it disagrees with
/// `codegen.toml`.
fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// The public reference packages; see `fixtures.rs`.
fn corpus() -> Vec<PathBuf> {
    crate::fixtures::public_corpus()
}

fn skip_if_empty(files: &[PathBuf], what: &str) -> bool {
    if files.is_empty() {
        eprintln!("skipped: no EPC under testdata/ ({what})");
        return true;
    }
    false
}

/// Every object whose type name ends in `Property`, in one package.
fn properties(store: &EpcStore) -> Vec<(String, Box<dyn energyml_core::registry::DynObject>)> {
    let mut out = Vec::new();
    for summary in store.resources() {
        if !summary.qualified_type.ends_with("Property") {
            continue;
        }
        if let Ok(object) = store.get_object(&summary.uri) {
            out.push((summary.uuid.clone(), object));
        }
    }
    out
}

#[test]
fn every_property_of_the_corpus_reads() {
    let files = corpus();
    if skip_if_empty(&files, "every_property_of_the_corpus_reads") {
        return;
    }
    let registry = registry();
    let mut total = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for (uuid, object) in properties(&store) {
            let property = property::read(object.as_reflect())
                .unwrap_or_else(|e| panic!("{}: {uuid} did not read: {e}", path.display()));
            total += 1;

            // The two fields nothing downstream can do without.
            assert_eq!(property.uuid.to_ascii_lowercase(), uuid.to_ascii_lowercase());
            assert!(
                !property.patches.is_empty(),
                "{}: {uuid} has no values at all, which the schema forbids (1..∞)",
                path.display()
            );
            // A property with no supporting representation is unattached data;
            // the schema makes it mandatory in both versions.
            assert!(
                property.supporting_representation.is_some(),
                "{}: {uuid} names no supporting representation",
                path.display()
            );
            assert!(
                property.values_per_element() >= 1,
                "{}: {uuid} claims zero values per element",
                path.display()
            );
        }
    }
    assert!(total > 0, "the corpus holds properties; none were read");
    eprintln!("{total} properties read across {} packages", files.len());
}

#[test]
fn the_two_versions_of_the_same_property_agree() {
    // `testingPackageCpp.epc` (2.0.1) and `testingPackageCpp22.epc` (2.2) are
    // the same model written to two schemas, with the uuids preserved. What a
    // version-agnostic reader is *for* is that these come out the same.
    let files = corpus();
    if skip_if_empty(&files, "the_two_versions_of_the_same_property_agree") {
        return;
    }
    let registry = registry();
    let (Some(old), Some(new)) = (
        files.iter().find(|p| p.ends_with("testingPackageCpp.epc")),
        files.iter().find(|p| p.ends_with("testingPackageCpp22.epc")),
    ) else {
        eprintln!("skipped: the two testingPackageCpp packages are not both in the corpus");
        return;
    };

    let old = EpcStore::open(old, registry.clone()).expect("the 2.0.1 package opens");
    let new = EpcStore::open(new, registry.clone()).expect("the 2.2 package opens");
    let mut compared = 0usize;

    for (uuid, object) in properties(&old) {
        let Ok(twin) = new.get_object(&uuid) else {
            continue;
        };
        let a = property::read(object.as_reflect()).expect("the 2.0.1 property reads");
        let b = property::read(twin.as_reflect()).expect("the 2.2 property reads");
        compared += 1;

        assert_eq!(a.indexable, b.indexable, "{uuid}: indexable element");
        assert_eq!(a.title, b.title, "{uuid}: title");
        // Not compared for equality, and the reason is a corpus fact worth
        // writing down: the two packages are *not* a perfect mirror. fesapi's
        // `6b6656bb` — titled "Fake Property" — hangs on a
        // `PointSetRepresentation` in the 2.0.1 package and on an
        // `IjkGridRepresentation` in the 2.2 one. Asserting equality here
        // would pin a producer's inconsistency as if it were our contract.
        assert!(
            a.supporting_representation.is_some() && b.supporting_representation.is_some(),
            "{uuid}: the schema makes the supporting representation mandatory in both versions"
        );
        assert_eq!(
            a.values_per_element(),
            b.values_per_element(),
            "{uuid}: 2.0.1 `Count` and 2.2 `ValueCountPerIndexableElement` must mean the same thing"
        );
        assert_eq!(a.patches.len(), b.patches.len(), "{uuid}: patch count");
        // The version difference that matters: 2.0.1 says which patch, 2.2
        // implies it from the order. Either way the answer is the same number.
        for (left, right) in a.patches.iter().zip(&b.patches) {
            assert_eq!(left.patch(), right.patch(), "{uuid}: patch attachment");
        }
    }
    assert!(compared > 0, "no property is in both packages");
    eprintln!("{compared} properties compared across the two versions");
}

#[test]
fn a_patch_index_is_read_where_it_is_written_and_implied_where_it_is_not() {
    let files = corpus();
    if skip_if_empty(
        &files,
        "a_patch_index_is_read_where_it_is_written_and_implied_where_it_is_not",
    ) {
        return;
    }
    let registry = registry();
    let mut explicit = 0usize;
    let mut positional = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for (_, object) in properties(&store) {
            let property = property::read(object.as_reflect()).expect("it reads");
            for (position, patch) in property.patches.iter().enumerate() {
                assert_eq!(patch.position, position);
                match patch.patch_index {
                    Some(_) => explicit += 1,
                    None => positional += 1,
                }
            }
        }
    }
    // Both forms are in the corpus, so both branches are actually exercised —
    // a test that only ever sees one of them proves nothing about the other.
    assert!(
        explicit > 0,
        "no 2.0.1 property carries a RepresentationPatchIndex"
    );
    assert!(positional > 0, "no 2.2 property attaches by position");
}

#[test]
fn the_kind_is_read_in_both_of_its_shapes() {
    let files = corpus();
    if skip_if_empty(&files, "the_kind_is_read_in_both_of_its_shapes") {
        return;
    }
    let registry = registry();
    let mut standard = 0usize;
    let mut referenced = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for (uuid, object) in properties(&store) {
            let property = property::read(object.as_reflect()).expect("it reads");
            match property.kind {
                Some(PropertyKindRef::Standard { ref name }) => {
                    assert!(!name.is_empty(), "{uuid}: an empty standard kind name");
                    standard += 1;
                }
                Some(PropertyKindRef::Object { ref uuid, .. }) => {
                    assert!(!uuid.is_empty(), "an empty kind reference");
                    referenced += 1;
                }
                None => panic!("{}: {uuid} names no property kind at all", path.display()),
            }
        }
    }
    assert!(
        standard > 0,
        "the 2.0.1 `StandardPropertyKind` form is never exercised"
    );
    assert!(referenced > 0, "the referenced form is never exercised");
}

#[test]
fn the_element_count_is_computed_where_it_is_known_and_refused_where_it_is_not() {
    let files = corpus();
    if skip_if_empty(
        &files,
        "the_element_count_is_computed_where_it_is_known_and_refused_where_it_is_not",
    ) {
        return;
    }
    let registry = registry();
    let mut known = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for (uuid, object) in properties(&store) {
            let property = property::read(object.as_reflect()).expect("it reads");
            let Some(support) = property.supporting_representation.as_deref() else {
                continue;
            };
            let Ok(representation) = store.get_object(support) else {
                continue;
            };
            match property::element_count(representation.as_reflect(), property.indexable) {
                Ok(count) => {
                    assert!(count > 0, "{}: {uuid} hangs on zero elements", path.display());
                    known += 1;
                }
                Err(e) => {
                    // A refusal must say which pair it refused: "not supported"
                    // with no names is what sends someone reading the source.
                    let message = e.to_string();
                    assert!(
                        message.contains(property.indexable.as_str()),
                        "a refusal that does not name the element: {message}"
                    );
                }
            }
        }
    }
    assert!(
        known > 0,
        "no element count could be computed anywhere in the corpus"
    );
}

#[test]
fn a_grid_2d_counts_its_nodes_the_same_way_in_both_versions() {
    // The count that does the most damage, on the one representation type
    // whose two versions spell it differently: 2.2 puts the axis counts on the
    // representation, 2.0.1 inside a `Grid2dPatch`.
    let files = corpus();
    if skip_if_empty(&files, "a_grid_2d_counts_its_nodes_the_same_way_in_both_versions") {
        return;
    }
    let registry = registry();
    let mut seen = Vec::new();

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for summary in store.resources() {
            if !summary.qualified_type.contains("Grid2dRepresentation") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let nodes = property::element_count(object.as_reflect(), IndexableElement::Nodes)
                .unwrap_or_else(|e| panic!("{}: {} has no node count: {e}", path.display(), summary.uuid));
            assert!(nodes > 0);
            seen.push((summary.uuid.to_ascii_lowercase(), nodes));
        }
    }

    // The same grid in the 2.0.1 and the 2.2 package must count the same.
    seen.sort();
    for pair in seen.windows(2) {
        if pair[0].0 == pair[1].0 {
            assert_eq!(
                pair[0].1, pair[1].1,
                "{}: two versions disagree on the node count",
                pair[0].0
            );
        }
    }
    assert!(!seen.is_empty(), "no Grid2dRepresentation in the corpus");
}

#[test]
#[cfg(feature = "hdf5")]
fn the_values_come_out_and_their_shape_is_stated() {
    // The end of the chain: the numbers themselves, and the count the document
    // implies stated beside the count the file holds. Where they disagree the
    // disagreement is *reported*, not hidden and not fatal — the reference
    // package has a real one (a 5000-node grid carrying 5000 values while
    // declaring 2 per node), and refusing it would make the file unusable
    // rather than repairable.
    let files = corpus();
    if skip_if_empty(&files, "the_values_come_out_and_their_shape_is_stated") {
        return;
    }
    let registry = registry();
    let mut read = 0usize;
    let mut disagreements = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for (uuid, object) in properties(&store) {
            let property = property::read(object.as_reflect()).expect("it reads");
            let mut total = 0usize;
            for patch in &property.patches {
                match patch.read(&store) {
                    Ok(values) => total += values.len(),
                    // A missing companion file is not this test's subject.
                    Err(e) if e.is_missing_file() => continue,
                    Err(e) => panic!("{}: {uuid} patch {}: {e}", path.display(), patch.position),
                }
            }
            if total == 0 {
                continue;
            }
            read += 1;

            let Some(support) = property.supporting_representation.as_deref() else {
                continue;
            };
            let Ok(representation) = store.get_object(support) else {
                continue;
            };
            for discrepancy in property.check(representation.as_reflect(), Some(total as u64)) {
                if matches!(discrepancy, property::Discrepancy::CountUnknown { .. }) {
                    continue;
                }
                disagreements += 1;
                eprintln!("{}: {uuid} {discrepancy}", path.display());
            }
        }
    }
    assert!(read > 0, "no property values were read at all");
    eprintln!("{read} properties read, {disagreements} whose declared and actual counts differ");
}

#[test]
fn the_index_finds_the_properties_of_a_representation_without_any_rels() {
    // `80wells` carries no `.rels` at all — 202 objects, not one relationship
    // part — which is exactly why the index is built from what the objects say
    // rather than from the package's own bookkeeping. If this test passes on
    // that file, the index does not secretly depend on the relationships.
    let files = corpus();
    if skip_if_empty(
        &files,
        "the_index_finds_the_properties_of_a_representation_without_any_rels",
    ) {
        return;
    }
    let registry = registry();
    let mut checked = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        let objects: Vec<Box<dyn energyml_core::registry::DynObject>> = store
            .resources()
            .iter()
            .filter_map(|summary| store.get_object(&summary.uri).ok())
            .collect();
        let (index, refused) = property::Index::build(objects.iter().map(|o| o.as_reflect()));
        assert!(refused.is_empty(), "{}: {refused:?}", path.display());

        // Every property the store holds is in the index, under the
        // representation it names.
        for (uuid, object) in properties(&store) {
            let read = property::read(object.as_reflect()).expect("it reads");
            let support = read.supporting_representation.as_deref().expect("mandatory");
            assert!(
                index.of(support).iter().any(|entry| entry.uuid == uuid),
                "{}: {uuid} is not indexed under {support}",
                path.display()
            );
            checked += 1;
        }
        assert!(
            index.orphans().is_empty(),
            "{}: a property with no supporting representation is kept, not dropped: {:?}",
            path.display(),
            index.orphans()
        );
    }
    assert!(checked > 0);
}

#[test]
fn the_index_separates_the_time_steps_of_one_representation() {
    // The case an index earns its keep on: several properties of the same kind
    // on the same grid, differing only by time index.
    let files = corpus();
    if skip_if_empty(&files, "the_index_separates_the_time_steps_of_one_representation") {
        return;
    }
    let registry = registry();
    let Some(path) = files.iter().find(|p| p.ends_with("testingPackageCpp22.epc")) else {
        eprintln!("skipped: testingPackageCpp22.epc is not in the corpus");
        return;
    };
    let store = EpcStore::open(path, registry).expect("the package opens");
    let objects: Vec<_> = store
        .resources()
        .iter()
        .filter_map(|summary| store.get_object(&summary.uri).ok())
        .collect();
    let (index, _) = property::Index::build(objects.iter().map(|o| o.as_reflect()));

    let timed: Vec<_> = index
        .representations()
        .into_iter()
        .flat_map(|uuid| index.of(uuid))
        .filter(|entry| entry.time.is_some())
        .collect();
    assert!(!timed.is_empty(), "the package carries time series properties");

    for entry in &timed {
        let Some(time) = &entry.time else { continue };
        let Some(step) = time.index else { continue };
        let support = index
            .representations()
            .into_iter()
            .find(|uuid| index.of(uuid).iter().any(|e| e.uuid == entry.uuid))
            .expect("it is indexed somewhere")
            .to_string();
        assert!(
            index
                .by_time(&support, step)
                .iter()
                .any(|found| found.uuid == entry.uuid),
            "{} is not found at its own time step {step}",
            entry.uuid
        );
    }
}

/// A property on a grid connection set counts **cell pairs**, not cells.
///
/// The rule is the Usage Guide's, §9.16.1, and it is a recommendation with a
/// number in it: *"Attach properties on a grid connection set representation
/// using a 'face' indexable element […] By 'face' it's meant the contact area
/// between each cell pair. Consequently the dataset corresponding to the
/// properties should have the same number of values as the number of cell
/// pairs."*
///
/// The trap is the other pair: a connection names **two** cells, so answering
/// the connection count for `cells` would line a per-cell array up against half
/// the elements it belongs to — the right length for the wrong elements, which
/// no downstream check would catch.
#[test]
fn a_connection_set_counts_its_pairs_on_the_faces_and_refuses_the_cells() {
    use energyml_domain::property::{self, IndexableElement};
    use energyml_resqml_2_0_1::GridConnectionSetRepresentation;

    let set = GridConnectionSetRepresentation {
        uuid: "44444444-4444-4444-4444-444444444444".to_string(),
        count: 12,
        ..Default::default()
    };

    assert_eq!(
        property::element_count(
            &set as &dyn energyml_core::value::Reflect,
            IndexableElement::Faces
        )
        .ok(),
        Some(12),
        "the guide's own number: one value per cell pair"
    );

    let refused = property::element_count(
        &set as &dyn energyml_core::value::Reflect,
        IndexableElement::Cells,
    )
    .unwrap_err()
    .to_string();
    assert!(refused.contains("cells"), "{refused}");
    assert!(
        refused.contains("GridConnectionSetRepresentation"),
        "a refusal must name the pair it refused: {refused}"
    );
}

/// A K gap moves two counts and leaves a third alone.
///
/// The distinction is the guide's, and it is the whole reason `layers` and
/// `intervals` are different indexable elements: *"the interval ordering is an
/// expanded version of the layer ordering, with the gaps interleaved between
/// the layers. Specifically, if there are NK layers and GapCount gaps between
/// layers, there will be NK + GapCount intervals."* Table 7 gives the boundary
/// count on the next row, `NKL = NK + GapCount + 1`, and the XSD annotation on
/// `KGaps` repeats both.
///
/// What this has to catch is the reading that treats a gap as invisible. Before
/// this, `interval edges` answered `NK + 1` — right for the unbroken grid a
/// synthetic fixture would have, wrong for exactly the grids a gap exists to
/// describe, and inconsistent with the node count computed a few lines below it
/// from the same NKL.
#[test]
fn a_k_gap_lengthens_the_intervals_and_their_edges_but_not_the_layers() {
    use energyml_domain::property::{self, IndexableElement::*};
    use energyml_resqml_2_0_1::{IjkGridRepresentation, KGaps};

    let mut grid = IjkGridRepresentation {
        uuid: "55555555-5555-5555-5555-555555555555".to_string(),
        ni: 4,
        nj: 3,
        nk: 5,
        ..Default::default()
    };
    let count = |grid: &IjkGridRepresentation, element| {
        property::element_count(grid as &dyn energyml_core::value::Reflect, element)
            .expect("the pair is known")
    };

    // Without a gap the two readings coincide, which is why a fixture with no
    // gap in it could not have found the defect.
    assert_eq!(count(&grid, Layers), 5);
    assert_eq!(count(&grid, Intervals), 5);
    assert_eq!(count(&grid, IntervalEdges), 6);

    grid.k_gaps = Some(KGaps {
        count: Some(2),
        ..Default::default()
    });

    assert_eq!(count(&grid, Layers), 5, "a gap is not a layer");
    assert_eq!(count(&grid, Intervals), 7, "NK + GapCount");
    assert_eq!(count(&grid, IntervalEdges), 8, "NKL = NK + GapCount + 1");
    // And the count that was already gap-aware, so the two now agree by
    // construction rather than by coincidence: one node per coordinate line per
    // layer boundary.
    assert_eq!(
        count(&grid, Nodes),
        count(&grid, CoordinateLines) * count(&grid, IntervalEdges),
        "the nodes are the coordinate lines times NKL"
    );
}

/// The counts that are local to a cell, and the one that is not.
///
/// Table 7 and §9.12.3.1 agree on both halves. The local ones are fixed by the
/// cell's own shape — *"for an IJK grid, there are 6 'faces per cell': 0=top,
/// 1=bottom, 2-5 are side faces"* — so no faulting can change them. The column
/// edges are the opposite kind, and the guide singles them out as *"the only
/// indexable element where the commonality in indexing … is not obvious"*:
/// *"first follows the I column edges (NIL x NJ) and then the J column edges
/// (NI x NJL)"*.
#[test]
fn the_per_cell_counts_are_local_and_the_column_edges_are_not() {
    use energyml_domain::property::{self, IndexableElement::*};
    use energyml_resqml_2_0_1::IjkGridRepresentation;

    let grid = IjkGridRepresentation {
        uuid: "66666666-6666-6666-6666-666666666666".to_string(),
        ni: 4,
        nj: 3,
        nk: 5,
        ..Default::default()
    };
    let count = |element| {
        property::element_count(&grid as &dyn energyml_core::value::Reflect, element)
            .expect("the pair is known")
    };

    assert_eq!(count(Cells), 60);
    assert_eq!(count(FacesPerCell), 6 * 60);
    assert_eq!(count(NodesPerCell), 8 * 60);
    assert_eq!(count(EdgesPerColumn), 4 * 12);
    // NIL x NJ + NI x NJL = 5*3 + 4*4, and not the 2*NI*NJ a reading that
    // forgot the outer edges would give.
    assert_eq!(count(ColumnEdges), 31);
    assert_ne!(count(ColumnEdges), 2 * 4 * 3);
}

/// A fault adds column edges, and the element that carries them moved between
/// releases.
///
/// Table 7 gives the IJK column edges as `NIL x NJ + NI x NJL +
/// SplitColumnEdgeCount`, so the split term is not a refinement — it is part of
/// the count, and a grid with a fault is precisely the grid someone indexes
/// properties on. `SplitColumnEdges` is spelled the same in both releases but
/// does not hang in the same place: 2.2 puts it on the grid geometry, 2.0.1
/// nests it inside the split coordinate lines. Reading only one place answers
/// the unfaulted number on a faulted grid.
#[test]
fn the_split_column_edges_are_found_wherever_the_release_hid_them() {
    use energyml_domain::property::{self, IndexableElement::ColumnEdges};

    let unfaulted = energyml_resqml_2_0_1::IjkGridRepresentation {
        uuid: "77777777-7777-7777-7777-777777777777".to_string(),
        ni: 4,
        nj: 3,
        nk: 5,
        ..Default::default()
    };
    let plain = property::element_count(&unfaulted as &dyn energyml_core::value::Reflect, ColumnEdges)
        .expect("the pair is known");
    assert_eq!(plain, 31);

    // 2.0.1: nested one level deeper, inside the split coordinate lines.
    let mut old = unfaulted.clone();
    old.geometry = Some(energyml_resqml_2_0_1::IjkGridGeometry {
        split_coordinate_lines: Some(energyml_resqml_2_0_1::ColumnLayerSplitCoordinateLines {
            count: 3,
            split_column_edges: Some(energyml_resqml_2_0_1::ColumnLayerSplitColumnEdges {
                count: 7,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    });
    assert_eq!(
        property::element_count(&old as &dyn energyml_core::value::Reflect, ColumnEdges).expect("known"),
        plain + 7,
        "2.0.1 hides the split column edges under the split coordinate lines"
    );

    // 2.2: straight on the geometry.
    let new = energyml_resqml_2_2::IjkGridRepresentation {
        uuid: "88888888-8888-8888-8888-888888888888".to_string(),
        ni: 4,
        nj: 3,
        nk: 5,
        geometry: Some(energyml_resqml_2_2::IjkGridGeometry {
            split_column_edges: Some(energyml_resqml_2_2::SplitColumnEdges {
                count: 7,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        property::element_count(&new as &dyn energyml_core::value::Reflect, ColumnEdges).expect("known"),
        plain + 7,
        "2.2 puts them straight on the geometry, and both releases must count alike"
    );
}

/// A truncated grid's cells are its parent range plus its truncation patch.
///
/// Table 7: *"cells … NI x NJ x NK (+) TruncationCellCount"*, where the guide's
/// `(+)` means *"this portion of the cell or node data is stored as a separate
/// patch"*. Answering `NI x NJ x NK` alone — which is what this did — is the
/// parent range with the second patch dropped, so every property indexed on the
/// cells of a truncated grid was measured against a short ruler.
///
/// The field moved between releases: 2.0.1 calls it `TruncationCells`, 2.2
/// `TruncationCellPatch`, with `TruncationCellCount` inside either.
#[test]
fn a_truncated_grid_counts_its_second_patch_of_cells() {
    use energyml_domain::property::{self, IndexableElement::Cells};

    let mut grid = energyml_resqml_2_0_1::TruncatedIjkGridRepresentation {
        uuid: "99999999-9999-9999-9999-999999999999".to_string(),
        ni: 4,
        nj: 3,
        nk: 5,
        ..Default::default()
    };
    let count = |grid: &energyml_resqml_2_0_1::TruncatedIjkGridRepresentation| {
        property::element_count(grid as &dyn energyml_core::value::Reflect, Cells).expect("the pair is known")
    };
    assert_eq!(count(&grid), 60, "an empty patch: the parent range alone");

    grid.truncation_cells = energyml_resqml_2_0_1::TruncationCellPatch {
        truncation_cell_count: 9,
        ..Default::default()
    };
    assert_eq!(
        count(&grid),
        69,
        "the parent range keeps its indices and the patch is appended"
    );
}

/// An areal gap adds pillars, and therefore coordinate lines, and therefore nodes.
///
/// The Usage Guide §9.12.2 says exactly what `IjGaps` changes and what it does
/// not: *"This option does not modify the indexing of the cells or columns, but
/// it **does increase the number of pillars and coordinate lines**."* Both
/// halves are asserted, because getting only the first right is the plausible
/// half-fix — and a cell count that moved would be far more visible than a node
/// count that is quietly short.
///
/// What made this worth a second look: Table 7 gives `pillars` and `coordinate
/// lines` the *same* leading term, which reads as a copy-paste in the PDF until
/// this sentence explains that the second genuinely builds on the first.
#[test]
fn an_areal_gap_adds_pillars_and_lines_but_not_cells() {
    use energyml_domain::property::{self, IndexableElement::*};
    use energyml_resqml_2_0_1::{IjGaps, IjkGridGeometry, IjkGridRepresentation};

    let mut grid = IjkGridRepresentation {
        uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string(),
        ni: 4,
        nj: 3,
        nk: 5,
        ..Default::default()
    };
    let count = |grid: &IjkGridRepresentation, element| {
        property::element_count(grid as &dyn energyml_core::value::Reflect, element)
            .expect("the pair is known")
    };

    assert_eq!(count(&grid, Pillars), 20);
    assert_eq!(count(&grid, CoordinateLines), 20);
    assert_eq!(count(&grid, Nodes), 20 * 6);

    grid.geometry = Some(IjkGridGeometry {
        ij_gaps: Some(IjGaps {
            split_pillar_count: Some(7),
            ..Default::default()
        }),
        ..Default::default()
    });

    assert_eq!(count(&grid, Pillars), 27, "NIL x NJL + SplitPillarCount");
    assert_eq!(count(&grid, CoordinateLines), 27, "the lines follow the pillars");
    assert_eq!(
        count(&grid, Nodes),
        27 * 6,
        "one node per line per layer boundary"
    );
    // And the half the guide is equally explicit about: the gap changes nothing
    // for the cells or the columns.
    assert_eq!(count(&grid, Cells), 60);
    assert_eq!(count(&grid, Columns), 12);
}
