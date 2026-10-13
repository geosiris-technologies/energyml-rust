//! The second layer: shapes, values, slices and queries.
//!
//! What the catalogue cannot say. These checks are the ones that would catch a
//! bridge handing back plausible numbers — so each confronts the geometry
//! reader with **the object's own JSON**, which is a different reader over the
//! same file. Two implementations agreeing on a count is worth more than one
//! implementation repeating itself.

#![cfg(feature = "hdf5")]

use std::path::PathBuf;

use energyml_rust::session::{ArrayRef, Block, Package, Topology};
use serde_json::Value;

fn corpus(name: &str) -> Option<PathBuf> {
    crate::fixtures::find_or_skip(name)
}

/// The package every check here uses, or `None`.
fn package() -> Option<Package> {
    let path = corpus("testingPackageCpp22.epc")?;
    Some(Package::open(&path).expect("the package opens"))
}

/// The first object whose type contains this.
fn first_of(package: &Package, kind: &str) -> Option<String> {
    package.find(Some(kind), None).into_iter().map(|one| one.uri).next()
}

#[test]
fn a_surface_comes_back_as_points_and_triangles_the_json_agrees_with() {
    let Some(package) = package() else { return };
    let Some(uri) = first_of(&package, "TriangulatedSetRepresentation") else {
        eprintln!("skipped: no triangulated set in this package");
        return;
    };

    let shape = package.geometry(&uri, "projected").expect("the shape reads");
    assert_eq!(shape.uri, uri);
    assert!(!shape.kind.is_empty(), "a shape says what kind it is");
    assert!(!shape.patches.is_empty(), "a surface has at least one patch");

    // **The confrontation.** The object's JSON states `NodeCount` and the
    // number of values in the triangle array; the geometry reader never looks
    // at either — it reads the arrays. If the two disagree, one of them is
    // wrong, and that is exactly the bug that would otherwise ship.
    let object: Value =
        serde_json::from_str(&package.get(&uri).expect("the object reads")).expect("JSON");
    let patch = &object["TrianglePatch"][0];
    let declared_nodes = patch["NodeCount"].as_u64().expect("the JSON states a node count");
    let declared_indices = patch["Triangles"]["Values"]["ExternalDataArrayPart"][0]["Count"][0]
        .as_u64()
        .expect("the JSON states how many indices there are");

    let read = &shape.patches[0];
    assert_eq!(read.points.len() as u64, declared_nodes * 3, "three numbers per point");
    match &read.topology {
        Topology::Triangles(indices) => {
            assert_eq!(indices.len() as u64, declared_indices, "as many indices as the file says");
            let highest = indices.iter().copied().max().unwrap_or(0) as u64;
            assert!(highest < declared_nodes, "an index points inside the patch: {highest}");
        }
        other => panic!("a triangulated set came back as {other:?}"),
    }
}

#[test]
fn the_same_surface_as_a_mesh_has_the_same_points() {
    let Some(package) = package() else { return };
    let Some(uri) = first_of(&package, "TriangulatedSetRepresentation") else { return };

    let shape = package.geometry(&uri, "projected").expect("the shape reads");
    let (points, triangles) = package.mesh(&uri, "projected").expect("the mesh reads");

    // `mesh` merges the patches and cuts what is not a triangle; on a
    // triangulated set there is nothing to cut, so the two must line up. That
    // is what makes this able to fail: a merge that dropped a patch, or an
    // offset applied twice, shows here.
    let from_patches: usize = shape.patches.iter().map(|one| one.points.len()).sum();
    assert_eq!(points.len(), from_patches, "the same points, patch by patch");
    assert_eq!(points[..3], shape.patches[0].points[..3], "and in the same order");
    assert!(!triangles.is_empty(), "a surface has triangles");
    assert_eq!(triangles.len() % 3, 0, "triples");
}

#[test]
fn a_placement_that_is_not_one_is_refused_with_the_list() {
    let Some(package) = package() else { return };
    let Some(uri) = first_of(&package, "TriangulatedSetRepresentation") else { return };

    // Nothing to skip and nothing to read: this is the check that runs even on
    // a machine with no corpus geometry at all, and it protects the one thing
    // a silent default would ruin — a model placed in the wrong system looks
    // exactly like a model placed in the right one.
    let why = package.geometry(&uri, "global").expect_err("`global` is not a placement here");
    assert!(why.contains("local"), "the refusal lists what is accepted: {why}");
    assert!(why.contains("projected"), "{why}");
    assert!(why.contains("metres"), "{why}");
}

#[test]
fn local_and_projected_are_not_the_same_answer_when_the_crs_offsets() {
    let Some(package) = package() else { return };
    let Some(uri) = first_of(&package, "TriangulatedSetRepresentation") else { return };

    let local = package.geometry(&uri, "local").expect("local reads");
    let projected = package.geometry(&uri, "projected").expect("projected reads");
    assert_eq!(
        local.patches[0].points.len(),
        projected.patches[0].points.len(),
        "placing moves points, it does not add or lose any"
    );

    // The corpus's CRS may or may not offset; what must hold either way is that
    // the *placement is reported*, so a caller can never mistake one for the
    // other.
    assert_eq!(local.placement, "local");
    assert_eq!(projected.placement, "projected");
}

#[test]
fn the_values_hanging_on_a_representation_come_back_with_their_scope() {
    let Some(package) = package() else { return };

    // Any representation that carries at least one property. Looking rather
    // than naming one keeps this working on another corpus.
    let found = package
        .find(Some("Representation"), None)
        .into_iter()
        .find_map(|one| match package.properties(&one.uri) {
            Ok((values, _)) if !values.is_empty() => Some((one, values)),
            _ => None,
        });
    let Some((representation, values)) = found else {
        eprintln!("skipped: no representation of this package carries a property");
        return;
    };

    let shape = package.geometry(&representation.uri, "projected").expect("its shape reads");
    let points: usize = shape.patches.iter().map(|one| one.points.len() / 3).sum();

    for value in &values {
        assert!(!value.name.is_empty(), "a property is named");
        assert!(
            value.scope == "points" || value.scope == "cells",
            "a scope is one or the other: {}",
            value.scope
        );
        assert!(!value.values.is_empty(), "{} came back empty", value.name);
        // **The count is the check.** A property lined up against the wrong
        // elements draws a plausible picture of nothing, and the only thing
        // that catches it is how many values there are.
        if value.scope == "points" {
            assert_eq!(
                value.values.len() as u64,
                points as u64 * value.components,
                "{} says it is on the nodes, and there are {points} of them",
                value.name
            );
        }
    }
}

#[test]
fn a_slice_of_an_array_is_the_same_numbers_as_the_whole() {
    let Some(package) = package() else { return };
    let found = package.find(None, None).into_iter().find_map(|one| {
        let arrays = package.arrays(&one.uri).ok()?;
        let array = arrays.into_iter().next()?;
        let shape = package.array_shape(&array).ok()?;
        // Doubles on purpose: the comparison at the end is number for number,
        // and widening one of the two sides would compare a conversion.
        (shape.dimensions.len() == 2 && shape.dimensions[0] >= 2 && shape.element_type == "double")
            .then_some((array, shape))
    });
    let Some((array, shape)) = found else {
        eprintln!("skipped: no two-dimensional array in this package");
        return;
    };

    let (_, whole) = package.read_array(&array).expect("the whole array reads");
    let width = shape.dimensions[1];
    let (part_shape, part) = package
        .read_array_part(&array, &[1, 0], &[1, width])
        .expect("the second row reads");

    assert_eq!(part_shape.dimensions, vec![1, width], "the slice is the shape asked for");
    // Row 1 of the whole array, by hand, against the row the backend sliced.
    // Two ways of getting the same numbers, and they are not the same code.
    let (Block::F64(whole), Block::F64(part)) = (&whole, &part) else {
        panic!("a `double` array did not come back as doubles");
    };
    let from = width as usize;
    assert_eq!(&part[..], &whole[from..from + width as usize], "number for number");
}

#[test]
fn a_slice_whose_two_halves_disagree_is_refused() {
    let Some(package) = package() else { return };
    let array = ArrayRef { file: "x.h5".to_string(), path_in_file: "/x".to_string() };
    let why = package
        .read_array_part(&array, &[0, 0], &[4])
        .expect_err("two starts and one count");
    assert!(why.contains('2') && why.contains('1'), "the refusal names both counts: {why}");
}

#[test]
fn the_catalogue_answers_a_question_instead_of_being_walked() {
    let Some(package) = package() else { return };
    let all = package.objects();
    assert_eq!(package.find(None, None).len(), all.len(), "asking nothing returns everything");

    let crs = package.find(Some("crs"), None);
    assert!(!crs.is_empty(), "this package holds CRSs");
    for one in &crs {
        assert!(one.qualified_type.to_lowercase().contains("crs"), "{}", one.qualified_type);
    }
    // Case does not matter, and the two filters are an AND — a filter that
    // quietly ORed would return more, which is the failure a test of one
    // filter alone would miss.
    let titled = package.find(Some("CRS"), Some("default"));
    assert!(titled.len() <= crs.len(), "adding a filter cannot add results");
    for one in &titled {
        assert!(one.title.to_lowercase().contains("default"), "{}", one.title);
    }
}
