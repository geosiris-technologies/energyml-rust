//! Geometry, against the packages it will actually be given.
//!
//! Skips when the reference packages are absent outside CI (`fixtures.rs`), like every other corpus test here. The
//! arrays live in HDF5, so most of this needs `--features hdf5`; what does not
//! need it is the part that matters most - a lattice carries no data, and its
//! points must come out of a build with no backend at all.

use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_geometry::parametric::Vertical;
use energyml_geometry::{Topology, VerticalQuantity, crs, geometry, points};
use energyml_storage::EpcStore;

// Both guards below serve tests that need a real `.h5`, so they are behind the
// same feature: without it they are dead code and clippy says so.
#[cfg(feature = "hdf5")]
/// Every schema version the workspace generates.
///
/// One line rather than a hand-kept subset: a list written out here goes stale
/// the day a release is added, and nothing would notice - the suite would keep
/// passing while quietly never covering the new version. `energyml-schemas`
/// holds the one list, and its own test fails when it disagrees with
/// `codegen.toml`.
/// Whether the object itself says it selects **no** element.
///
/// Asked of the object, never inferred from an empty result: an empty patch
/// because the file declared zero and an empty patch because the selection was
/// lost are the same picture and opposite news.
///
/// Read from the patches rather than through `property::element_count`, which
/// **refuses** a computed zero on purpose (05/09) - the right answer for a
/// property looking for elements to hang on, the wrong tool for asking a
/// document what it says. Here zero is the thing being looked for.
#[cfg(feature = "hdf5")]
fn declares_nothing(object: &dyn energyml_core::value::Reflect) -> bool {
    use energyml_core::value::{FieldValue, Scalar};
    let Some(FieldValue::List(patches)) = object.field("SubRepresentationPatch") else {
        return false;
    };
    if patches.is_empty() {
        return false;
    }
    patches.iter().all(|patch| {
        matches!(
            patch
                .as_object()
                .and_then(|p| p.field("Count"))
                .and_then(|f| f.as_scalar()),
            Some(Scalar::UInt(0)) | Some(Scalar::Int(0))
        )
    })
}

/// Whether this type's points are **copied** from another representation rather
/// than computed.
///
/// The one distinction the non-finite guard needs. Everything else in this crate
/// evaluates - a lattice, a spline, a pillar - and an evaluation yielding nothing
/// but `NaN` is arithmetic gone wrong. A sub-representation does no arithmetic:
/// it names indices into a support and hands back what is there, so an
/// all-non-finite patch is the support's no-data area, carried faithfully.
#[cfg(feature = "hdf5")]
fn selects_rather_than_computes(qualified_type: &str) -> bool {
    qualified_type.contains("SubRepresentation")
}

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// The public reference packages; see `fixtures.rs`.
fn corpus() -> Vec<PathBuf> {
    crate::fixtures::public_corpus()
}

fn skip(files: &[PathBuf], what: &str) -> bool {
    if files.is_empty() {
        eprintln!("skipped `{what}`: no reference package (scripts/fetch-fixtures.py)");
        return true;
    }
    false
}

#[test]
#[cfg(feature = "hdf5")]
fn every_supported_representation_yields_points() {
    let files = corpus();
    if skip(&files, "every_supported_representation_yields_points") {
        return;
    }
    let mut done: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut refused: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut total_points = 0usize;
    let mut absent = 0usize;
    let mut non_finite = 0usize;
    let mut malformed = 0usize;

    // A package whose `.h5` companion is not in the corpus is not a failure of
    // this crate - it is a gap in the corpus. Counted and printed, so a pass is
    // never mistaken for coverage. Same rule as the storage corpus test.
    let is_absent = |e: &energyml_geometry::Error| e.is_missing_file();

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            if !geometry::is_supported(object.as_reflect()) {
                continue;
            }
            match geometry::of(object.as_reflect(), &store) {
                Ok(geometry) => {
                    // The assertion that matters most, and the one that was
                    // missing: a *successful* evaluation yielding nothing is
                    // indistinguishable from a working reader with nothing to
                    // draw. It hid a whole RESQML 2.2 spelling - point sets
                    // whose geometry hangs on `NodePatchGeometry` rather than
                    // under a `NodePatch` - behind a passing test.
                    assert!(
                        !geometry.patches.is_empty(),
                        "{}: {} ({}) evaluated to no patch at all",
                        path.display(),
                        summary.uuid,
                        geometry.kind
                    );
                    assert!(
                        // **Unless the file says the selection is empty.** A
                        // sub-representation declares how many elements it
                        // picks, and nothing forbids zero: `h+g_PaleoGTM` has
                        // `17a1ed5e`, titled "SnS_adjust2000_bad_points", whose
                        // patch states `Count: 0` - an empty set of bad points,
                        // which is a sensible thing for a producer to write and
                        // for us to carry. Demanding a point here called that a
                        // failure of ours.
                        geometry.point_count() > 0 || declares_nothing(object.as_reflect()),
                        "{}: {} ({}) evaluated to {} patch(es) and no point",
                        path.display(),
                        summary.uuid,
                        geometry.kind,
                        geometry.patches.len()
                    );
                    for patch in &geometry.patches {
                        // A triangle index outside its own patch is the mistake
                        // that draws garbage instead of failing.
                        if let Topology::Triangles(triangles) = &patch.topology {
                            for triangle in triangles {
                                for index in triangle {
                                    assert!(
                                        (*index as usize) < patch.points.len(),
                                        "{}: {} triangle index {index} of {} points",
                                        path.display(),
                                        summary.uuid,
                                        patch.points.len()
                                    );
                                }
                            }
                        }
                        if let Topology::Grid { fastest, slowest } = patch.topology {
                            assert_eq!(fastest * slowest, patch.points.len());
                        }
                        // A NaN or an infinity here is either an evaluation
                        // bug - a lattice read wrong, a division by zero - or
                        // **data**: SISMAGE and Gocad write non-finite values
                        // as "no surface here" sentinels, and `ALWYN-RESQML`
                        // carries datasets that are 80-99 % sentinel. The two
                        // are told apart by volume: a bug makes *everything*
                        // non-finite (a bad lattice direction poisons every
                        // point), a sentinel leaves the footprint intact. So
                        // the guard is that some points are finite, and the
                        // sentinels are counted out loud rather than asserted
                        // away.
                        let finite = patch
                            .points
                            .iter()
                            .filter(|p| p.iter().all(|c| c.is_finite()))
                            .count();
                        if finite < patch.points.len() {
                            non_finite += patch.points.len() - finite;
                            // **Except where nothing was evaluated.** A
                            // sub-representation *selects* its support's
                            // points; it computes none. So an all-non-finite
                            // patch there says the selection fell inside the
                            // support's no-data area, which is a fact about the
                            // file and not arithmetic - `h+g_PaleoGTM` has one
                            // (`08d9c2d4`, "SnS_faults_proximity", 7 481 nodes)
                            // over a `Grid2d` whose z array the file itself
                            // opens with `NaN, NaN, NaN, …` and which is 89 %
                            // sentinel. Asserting here claimed a bug of ours
                            // where there was a producer's choice.
                            assert!(
                                finite > 0 || selects_rather_than_computes(&summary.qualified_type),
                                "{}: {} - every point of the patch is non-finite, \
                                 which is an evaluation bug, not a data sentinel",
                                path.display(),
                                summary.uuid
                            );
                        }
                    }
                    // The merged buffers are what a renderer uploads, and they
                    // are the one place `node_offset` is applied. An index that
                    // escapes here draws a mesh rather than failing.
                    let (merged_points, merged_triangles) = geometry.merged();
                    assert_eq!(merged_points.len(), geometry.point_count());
                    for triangle in &merged_triangles {
                        for index in triangle {
                            assert!(
                                (*index as usize) < merged_points.len(),
                                "{}: {} merged index {index} of {} points",
                                path.display(),
                                summary.uuid,
                                merged_points.len()
                            );
                        }
                    }
                    total_points += geometry.point_count();
                    *done.entry(geometry.kind.to_string()).or_default() += 1;
                }
                Err(energyml_geometry::Error::Unsupported { form, .. }) => {
                    *refused.entry(form.to_string()).or_default() += 1;
                }
                Err(e) if is_absent(&e) => absent += 1,
                // A named refusal of a defective real file is the library
                // doing its job - Volve ships an 891-byte
                // TriangulatedSetRepresentation with no patch at all. What
                // stays a failure is anything unnamed, and any malformed
                // refusal from the fesapi packages, which are conformant.
                // **A named refusal about the file, not about the reader.** A
                // horizontal plane states a depth and nothing about how far it
                // extends, so there is no shape to draw and no shape to guess.
                // It needed an arm of its own: the variant was added with the
                // plane reader and this test panics on anything it does not
                // name, so registering a reader that legitimately refuses part
                // of its input turned three corpus tests red - under `hdf5`
                // only, which is why it went unseen.
                Err(energyml_geometry::Error::Unbounded { form, .. }) => {
                    *refused.entry(form.to_string()).or_default() += 1;
                }
                Err(energyml_geometry::Error::Malformed { .. })
                    if !path.to_string_lossy().contains("testingPackageCpp") =>
                {
                    malformed += 1;
                    eprintln!(
                        "  malformed, refused by name: {} in {}",
                        summary.uuid,
                        path.display()
                    );
                }
                Err(e) => panic!("{}: {} - {e}", path.display(), summary.uuid),
            }
        }
    }

    eprintln!("{total_points} points, from {done:?}");
    if malformed > 0 {
        eprintln!("{malformed} representation(s) refused as malformed, by name");
    }
    if non_finite > 0 {
        eprintln!("{non_finite} non-finite point(s) - sentinel values carried by the source data");
    }
    if !refused.is_empty() {
        eprintln!("not evaluated yet: {refused:?}");
    }
    if absent > 0 {
        eprintln!("{absent} skipped: their package's `.h5` companion is not in testdata/");
    }
    assert!(total_points > 0, "not a single point came out of the corpus");
    assert!(done.len() >= 3, "only {done:?} - the corpus has more than that");
}

#[test]
fn a_lattice_needs_no_backend_at_all() {
    // The point of the whole module: a `Point3dLatticeArray` carries an origin
    // and directions, and its points are computed. A build with no HDF5
    // backend must still produce them - and a reader that only knew HDF5 would
    // return nothing here, which looks exactly like a working program.
    let files = corpus();
    if skip(&files, "a_lattice_needs_no_backend_at_all") {
        return;
    }
    let mut lattices = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            // Reach the point array without going through a representation, so
            // this works whatever the representation turns out to be.
            let Some(array) = find_lattice(object.as_reflect()) else {
                continue;
            };
            let evaluated =
                points::read(array, &store, "lattice", Vertical::Down).expect("a lattice evaluates");
            assert!(
                !evaluated.is_empty(),
                "{}: {} has a lattice that produced no point",
                path.display(),
                summary.uuid
            );
            assert!(evaluated.iter().all(|p| p.iter().all(|c| c.is_finite())));
            lattices += 1;
        }
    }
    eprintln!("{lattices} lattice point array(s) evaluated without any array backend");
    assert!(lattices > 0, "the corpus has lattices; none was found");
}

/// The first `Point3dLatticeArray` anywhere inside an object.
fn find_lattice(object: &dyn energyml_core::value::Reflect) -> Option<&dyn energyml_core::value::Reflect> {
    use energyml_core::value::FieldValue;
    if object.type_info().name == "Point3dLatticeArray" {
        return Some(object);
    }
    for field in object.type_info().fields {
        match object.field(field.name) {
            Some(FieldValue::Object(child)) => {
                if let Some(found) = find_lattice(child) {
                    return Some(found);
                }
            }
            Some(FieldValue::List(items)) => {
                for item in items {
                    if let Some(child) = item.as_object()
                        && let Some(found) = find_lattice(child)
                    {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn a_grid_comes_out_the_way_round_it_says_it_is() {
    // A transposed lattice has exactly the right number of points, every one of
    // them finite, and every count agreeing - and draws the surface sideways.
    // Nothing else in this file would notice, so this checks the one thing that
    // tells the two apart: the declared `FastestAxisCount` is how many points
    // go by before the step changes.
    let files = corpus();
    if skip(&files, "a_grid_comes_out_the_way_round_it_says_it_is") {
        return;
    }
    let mut checked = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            if !geometry::is_supported(object.as_reflect()) {
                continue;
            }
            let Ok(geometry) = geometry::of(object.as_reflect(), &store) else {
                continue;
            };
            for patch in &geometry.patches {
                let Topology::Grid { fastest, slowest } = patch.topology else {
                    continue;
                };
                // Equal axes cannot be told apart, and a grid that is not
                // regular has no single step to look for.
                if fastest == slowest || patch.points.len() < 3 {
                    continue;
                }
                // The comparison runs on x and y only: real z-value grids
                // carry NaN as a "no surface here" sentinel (79-99 % of some
                // ALWYN and Volve datasets), and NaN breaks any step
                // comparison that includes it. The lattice's regularity -
                // what a transposition would disturb - lives entirely in the
                // horizontal plane.
                let flat = |p: [f64; 3]| [p[0], p[1], 0.0];
                let step = sub(flat(patch.points[1]), flat(patch.points[0]));
                let mut run = 1usize;
                while run < patch.points.len()
                    && same(sub(flat(patch.points[run]), flat(patch.points[run - 1])), step)
                {
                    run += 1;
                }
                if run == patch.points.len() {
                    continue; // one straight line of points: nothing to compare
                }
                assert_eq!(
                    run,
                    fastest,
                    "{}: {} is {fastest} × {slowest} but the step changes after \
                     {run} point(s) - the lattice came out transposed",
                    path.display(),
                    summary.uuid
                );
                checked += 1;
            }
        }
    }
    eprintln!("{checked} non-square lattice grid(s) checked against their declared axes");
    assert!(
        checked > 0,
        "the corpus has non-square lattice grids; none was checked"
    );
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Two vectors equal to within what double precision can actually hold.
///
/// **Relative, not absolute**, and the difference is not academic. The absolute
/// `1e-9` this used to be is finer than the *representable granularity* of a
/// coordinate above about 10^7: one ulp at 10^10 is 2 × 10^-6, so a running sum
/// that is exact in intent differs from its own step by more than the tolerance
/// after a handful of terms. The comparison then reports "the step changed"
/// about arithmetic rather than about the lattice.
///
/// It came up on `h+g_PaleoGTM.epc`, whose copy of
/// grid `8d3c3099` states an axis 19 629 173 km long - the file is broken, as an
/// independent triangulation showed. But the fix is here and is
/// not about that file: a test that cannot survive large coordinates is a test
/// that will go red again on the next legitimately large model, and it would be
/// red for a reason that has nothing to do with what it is checking.
///
/// The absolute floor keeps it working around zero, where a relative tolerance
/// means nothing. A transposition is not a rounding difference - it changes the
/// step to another vector entirely - so nothing this exists to catch escapes.
fn same(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0))
}

#[test]
fn a_representation_knows_where_it_is() {
    let files = corpus();
    if skip(&files, "a_representation_knows_where_it_is") {
        return;
    }
    let mut with_crs = 0usize;
    let mut with_epsg = 0usize;
    let mut compound_with_epsg = 0usize;
    let mut stated_units = 0usize;
    let mut time_systems = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            if !geometry::is_supported(object.as_reflect()) {
                continue;
            }
            let Some(crs) = crs::of(object.as_reflect(), &store)
                .unwrap_or_else(|e| panic!("{}: {} - {e}", path.display(), summary.uuid))
            else {
                continue;
            };
            with_crs += 1;
            if crs.projected.epsg().is_some() {
                with_epsg += 1;
                // An eml 2.3 compound CRS keeps its EPSG code two references
                // away, in a `LocalEngineering2dCrs` that is its own data
                // object. Reading only the compound gives a CRS that looks
                // perfectly valid and says "unknown system", so this counts the
                // case that proves the references were followed.
                if crs.kind.contains("Compound") {
                    compound_with_epsg += 1;
                }
            }
            assert!(crs.areal_rotation.is_finite());
            // Placing a point must not turn it into a non-number.
            let placed = crs.to_projected([1.0, 2.0, 3.0]);
            assert!(
                placed.iter().all(|c| c.is_finite()),
                "{}: {placed:?}",
                path.display()
            );

            // Every unit the corpus actually states must be one this build can
            // convert. A CRS in feet read as metres is the bug that has no
            // symptom, so the test demands the conversion rather than
            // accepting whatever `to_projected` happened to return.
            //
            // The one legitimate refusal is a seismic time system, where the
            // third coordinate is a travel time and no length conversion
            // exists. The corpus has one, which is what makes the distinction
            // worth carrying in the type.
            match crs.to_metres([1.0, 2.0, 3.0]) {
                Ok(metric) => {
                    assert_eq!(
                        crs.vertical_quantity,
                        VerticalQuantity::Length,
                        "{}: a time CRS must not convert to metres",
                        path.display()
                    );
                    assert!(metric.iter().all(|c| c.is_finite()));
                    if crs.is_metric() {
                        assert_eq!(metric, placed, "a metric CRS must not move its own points");
                    } else {
                        stated_units += 1;
                    }
                }
                Err(_) if crs.vertical_quantity == VerticalQuantity::Time => {
                    time_systems += 1;
                }
                Err(e) => panic!(
                    "{}: {} states a unit this build cannot convert - {e}",
                    path.display(),
                    summary.uuid
                ),
            }
        }
    }
    eprintln!(
        "{with_crs} representation(s) with a CRS, {with_epsg} of them with an EPSG code \
         ({compound_with_epsg} through an eml 2.3 compound CRS), \
         {stated_units} in a unit other than the metre, {time_systems} seismic time system(s)"
    );
    assert!(with_crs > 0, "not one representation named its CRS");
    assert!(
        compound_with_epsg > 0,
        "the corpus has RESQML 2.2 packages whose compound CRS names EPSG:23031 two \
         references away; none was resolved"
    );
}

/// The fault in "two faulted sugar cubes" is honoured.
///
/// The reference package holds a 2×1×1 grid with one fault: pillars 1 and 4 are
/// split, and column 1 uses the split lines instead. The two cells are
/// therefore **not** joined across the middle - there is a 50 m throw.
///
/// This is the assertion that matters, because ignoring split coordinate lines
/// does not fail. It produces a grid that draws, with the cells on one side of
/// the fault pulled onto the other side's pillars: cell 1 would take two
/// corners from z = 300 and two from z = 350 and come out sheared. So the test
/// is that each cell's top face is **flat**, and that the two faces are at
/// different heights.
#[test]
#[cfg(feature = "hdf5")]
fn a_fault_keeps_the_two_cells_apart() {
    let mut checked = 0usize;

    for name in ["testingPackageCpp.epc", "testingPackageCpp22.epc"] {
        let Some(path) = crate::fixtures::find(name) else {
            continue;
        };
        let store = EpcStore::open(&path, registry()).expect("the package opens");
        for summary in store.resources() {
            if !summary.qualified_type.contains("IjkGrid") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let Ok(geometry) = geometry::of(object.as_reflect(), &store) else {
                continue;
            };
            let patch = &geometry.patches[0];
            let Topology::Hexahedra {
                cells, dimensions, ..
            } = &patch.topology
            else {
                continue;
            };
            // The faulted example, and only it: 2×1×1 with two cells.
            if *dimensions != [2, 1, 1] || cells.len() != 2 {
                continue;
            }

            let z_of = |cell: &[u32; 8], face: usize| -> Vec<f64> {
                (0..4)
                    .map(|corner| patch.points[cell[face * 4 + corner] as usize][2])
                    .collect()
            };
            for (index, cell) in cells.iter().enumerate() {
                for face in 0..2 {
                    let z = z_of(cell, face);
                    let first = z[0];
                    assert!(
                        z.iter().all(|value| (value - first).abs() < 1e-6),
                        "{name}: cell {index} face {face} is sheared: {z:?} - \
                         the split coordinate lines were not applied"
                    );
                }
            }
            // And the two cells sit at different depths, which is the fault.
            let (a, b) = (z_of(&cells[0], 0)[0], z_of(&cells[1], 0)[0]);
            assert!(
                (a - b).abs() > 1.0,
                "{name}: both cells are at z = {a}; the fault throw has been lost"
            );
            eprintln!("{name}: the fault throw is {:.1} m", (a - b).abs());
            checked += 1;
        }
    }

    // The packages are fetched, never versioned, so they are absent on a dev
    // machine that did not fetch them. Skipping is not passing: with the files present this
    // still refuses an empty run, which is the case the assertion exists for.
    if corpus().is_empty() {
        eprintln!("skipped: the corpus is not present");
        return;
    }
    assert!(
        checked > 0,
        "neither reference package yielded the 2×1×1 faulted grid; the test proves nothing"
    );
}

/// Every cell that comes out is a hexahedron VTK will accept.
#[test]
#[cfg(feature = "hdf5")]
fn every_cell_is_wound_the_way_vtk_wants() {
    let files = corpus();
    if skip(&files, "every_cell_is_wound_the_way_vtk_wants") {
        return;
    }
    let mut cells = 0usize;
    let mut grids = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            if !geometry::is_supported(object.as_reflect()) {
                continue;
            }
            let Ok(geometry) = geometry::of(object.as_reflect(), &store) else {
                continue;
            };
            for patch in &geometry.patches {
                let Topology::Hexahedra {
                    cells: found,
                    dimensions,
                    // The RESQML numbering, which this check does not look at:
                    // it is about the corners of the cells that came out, and a
                    // compacted grid renumbers without moving any of them.
                    index: _,
                } = &patch.topology
                else {
                    continue;
                };
                grids += 1;
                assert!(
                    found.len() <= dimensions[0] * dimensions[1] * dimensions[2],
                    "{}: more cells than the {dimensions:?} grid can hold",
                    path.display()
                );
                for cell in found {
                    for corner in cell {
                        assert!(
                            (*corner as usize) < patch.points.len(),
                            "{}: corner {corner} of {} points",
                            path.display(),
                            patch.points.len()
                        );
                    }
                    // VTK's rule: face (0,1,2,3)'s right-hand normal points at
                    // face (4,5,6,7). A cell built the other way round renders
                    // as a hole rather than failing, so it is checked here.
                    //
                    // Checked as the sign of the cell's volume, which is what
                    // that rule amounts to and stays defined where the rule's
                    // own arithmetic does not - see `ijk::oriented` for the
                    // corpus cell that broke the arithmetic.
                    //
                    // **What this does and does not prove.** The sum below is
                    // the same quantity the reader decides on, spelt as 24
                    // tetrahedra from the cell's centre rather than as an
                    // integral over its six faces. So it does not check the
                    // criterion - the ground truth for that is a cube whose
                    // right order is known in advance, and it lives in
                    // `ijk::tests`. What it checks is that every path which
                    // builds hexahedra actually orients them, on every grid the
                    // corpus has: `ijk`, `parent` for a local grid, and
                    // whatever is added next. Deleting the swap in `oriented`
                    // reddens it on the first grid the corpus offers, and
                    // reddens the two cube tests in `ijk::tests` with it.
                    //
                    // A five-tetrahedron decomposition is the cheaper spelling
                    // and would be a different quantity, not a second opinion:
                    // it picks a diagonal, and on the warped cells of
                    // `h+g_PaleoGTM` it calls 3 044 of them negative where the
                    // trilinear volume calls 12.
                    let point = |i: usize| patch.points[cell[i] as usize];
                    let mut centre = [0.0f64; 3];
                    for corner in 0..8 {
                        let p = point(corner);
                        for (axis, value) in centre.iter_mut().enumerate() {
                            *value += p[axis] / 8.0;
                        }
                    }
                    // Each face wound so its normal points out of the cell when
                    // the corners are in VTK's order.
                    const FACES: [[usize; 4]; 6] = [
                        [0, 3, 2, 1],
                        [4, 5, 6, 7],
                        [0, 1, 5, 4],
                        [1, 2, 6, 5],
                        [2, 3, 7, 6],
                        [3, 0, 4, 7],
                    ];
                    let mut volume = 0.0;
                    // The size of the cell, to judge the sign against: a pinched (zero-thickness)
                    // cell has a volume of zero up to rounding, whose sign differs from one platform
                    // to the next (s_bend.epc: -2.7e-14 on Linux), while an inside-out cell has a
                    // volume of the order of its extent cubed.
                    let mut extent = 0.0f64;
                    for axis in 0..3 {
                        let values = (0..8).map(|corner| point(corner)[axis]);
                        let (low, high) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
                            (l.min(v), h.max(v))
                        });
                        extent = extent.max(high - low);
                    }
                    for face in FACES {
                        let mut middle = [0.0f64; 3];
                        for corner in face {
                            let p = point(corner);
                            for (axis, value) in middle.iter_mut().enumerate() {
                                *value += p[axis] / 4.0;
                            }
                        }
                        for index in 0..4 {
                            let (a, b) = (point(face[index]), point(face[(index + 1) % 4]));
                            // The tetrahedron (centre, middle, a, b).
                            let u = [a[0] - centre[0], a[1] - centre[1], a[2] - centre[2]];
                            let v = [b[0] - centre[0], b[1] - centre[1], b[2] - centre[2]];
                            let w = [
                                middle[0] - centre[0],
                                middle[1] - centre[1],
                                middle[2] - centre[2],
                            ];
                            let normal = [
                                u[1] * v[2] - u[2] * v[1],
                                u[2] * v[0] - u[0] * v[2],
                                u[0] * v[1] - u[1] * v[0],
                            ];
                            let tetrahedron = (0..3).map(|i| normal[i] * w[i]).sum::<f64>() / 6.0;
                            volume += tetrahedron;
                        }
                    }
                    assert!(
                        volume >= -1e-9 * extent.powi(3),
                        "{}: a cell is inside out (volume = {volume})",
                        path.display()
                    );
                    cells += 1;
                }
            }
        }
    }

    eprintln!("{cells} cell(s) from {grids} IJK grid(s), all wound the way VTK asks");
    assert!(grids > 0, "no IJK grid was read at all");
}

/// An unstructured grid's faces all point out of their own cells.
///
/// The corpus grid is fesapi's `9283cd33`, *"One tetrahedron + prism grid"* -
/// two cells of different shapes sharing a face, which is exactly the case that
/// makes an unstructured grid unlike an IJK one.
///
/// Two things are checked, and the second is the one that matters.
///
/// The counts are arithmetic this test knows: 7 nodes, 8 faces, 2 cells, and
/// **9** cell-face pairs - one more than the face count, because face 0 belongs
/// to both cells. A reader that took `CellFaceIsRightHanded` as one flag per
/// face rather than per pair would need 8 and be refused.
///
/// Then every face is **measured**: its right-hand normal against the vector
/// from the cell's centroid to the face's. Outward means a positive dot
/// product, for all 9 pairs. That is an independent check of the whole chain -
/// the jagged arrays, the per-pair flag, and the reversal `faces_outward`
/// applies - because it re-derives from the coordinates what the flags claim.
///
/// It is done in the **projected** frame, and that is not a detail. This CRS is
/// `ZIncreasingDownward`, so the stored numbers form a left-handed frame and
/// every one of these normals points the *other* way in them. The flag is a
/// statement about physical space; the exporters flip z before writing, so what
/// lands in the file is right. Reading the flag in the local frame and
/// believing it would turn every cell inside out.
#[test]
#[cfg(feature = "hdf5")]
fn every_unstructured_face_points_out_of_its_cell() {
    let files = corpus();
    if skip(&files, "every_unstructured_face_points_out_of_its_cell") {
        return;
    }
    const GRID: &str = "9283cd33-5e52-4110-b7b1-616abde2b303";
    let mut checked = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        let Some(summary) = store.resources().iter().find(|s| s.uuid == GRID).cloned() else {
            continue;
        };
        let object = store.get_object(&summary.uri).expect("the grid reads");
        let found = geometry::of(object.as_reflect(), &store)
            .unwrap_or_else(|e| panic!("{}: the grid did not evaluate: {e}", path.display()));

        let crs = energyml_domain::crs::of(object.as_reflect(), &store)
            .expect("the CRS resolves")
            .expect("the grid names a CRS");

        let patch = &found.patches[0];
        let points: Vec<[f64; 3]> = patch.points.iter().map(|p| crs.to_projected(*p)).collect();
        assert_eq!(points.len(), 7, "{}: seven nodes", path.display());

        let Topology::Polyhedra(cells) = &patch.topology else {
            panic!("{}: an unstructured grid is polyhedra", path.display());
        };
        assert_eq!(cells.cell_count(), 2);
        assert_eq!(cells.face_count(), 8);
        assert_eq!(
            cells.slot_count(),
            9,
            "{}: 9 cell-face pairs for 8 faces - face 0 is shared",
            path.display()
        );

        for cell in 0..cells.cell_count() {
            // The cell's own nodes, whichever faces they came from.
            let mut nodes: Vec<u32> = cells
                .cell(cell)
                .flat_map(|(f, _)| cells.face(f).to_vec())
                .collect();
            nodes.sort_unstable();
            nodes.dedup();
            let centre = centroid(nodes.iter().map(|n| points[*n as usize]));

            for face in cells.faces_outward(cell) {
                let ring: Vec<[f64; 3]> = face.iter().map(|n| points[*n as usize]).collect();
                assert!(
                    ring.len() >= 3,
                    "{}: a face of {} node(s)",
                    path.display(),
                    ring.len()
                );
                let normal = newell(&ring);
                let middle = centroid(ring.iter().copied());
                let away = [
                    middle[0] - centre[0],
                    middle[1] - centre[1],
                    middle[2] - centre[2],
                ];
                let dot = normal[0] * away[0] + normal[1] * away[1] + normal[2] * away[2];
                assert!(
                    dot > 0.0,
                    "{}: cell {cell}'s face {face:?} has its normal turned inward (dot {dot})",
                    path.display()
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "the corpus holds `{GRID}`; it was not evaluated");
    eprintln!("{checked} cell-face pair(s) measured outward");
}

/// The average of some points.
#[cfg(feature = "hdf5")]
fn centroid(points: impl Iterator<Item = [f64; 3]>) -> [f64; 3] {
    let mut sum = [0.0f64; 3];
    let mut count = 0.0f64;
    for point in points {
        for axis in 0..3 {
            sum[axis] += point[axis];
        }
        count += 1.0;
    }
    sum.map(|total| total / count.max(1.0))
}

/// The normal of a ring of points, by Newell's method - right for a face that
/// is not quite planar, which a real cell face never is.
#[cfg(feature = "hdf5")]
fn newell(ring: &[[f64; 3]]) -> [f64; 3] {
    let mut normal = [0.0f64; 3];
    for index in 0..ring.len() {
        let current = ring[index];
        let next = ring[(index + 1) % ring.len()];
        normal[0] += (current[1] - next[1]) * (current[2] + next[2]);
        normal[1] += (current[2] - next[2]) * (current[0] + next[0]);
        normal[2] += (current[0] - next[0]) * (current[1] + next[1]);
    }
    normal
}

/// A local grid is its parent's window, subdivided.
///
/// The corpus's one local grid - fesapi's `2aec1720`, titled *LGR* - refines a
/// single cell of *"two faulted sugar cubes"* into 2 × 1 × 3. Until 01/08 it
/// was refused by name for having no `Geometry`, which was true and useless: a
/// local grid never has one, and its points are arithmetic on the parent's.
///
/// Checked against numbers this test knows without asking the library. The
/// parent cell is the box x ∈ [0, 375], y ∈ [0, 150], z ∈ [300, 500] - pillars
/// 0, 1, 3, 4 of the parent, none of which its two split lines touch. Cut 2 × 1
/// × 3 with no weights, the child's nodes can only be at x ∈ {0, 187.5, 375},
/// y ∈ {0, 150} and z ∈ {300, 366⅔, 433⅓, 500}.
#[test]
#[cfg(feature = "hdf5")]
fn a_local_grid_subdivides_its_parents_window() {
    let files = corpus();
    if skip(&files, "a_local_grid_subdivides_its_parents_window") {
        return;
    }
    const LGR: &str = "2aec1720-fa3e-11e5-a116-0002a5d5c51b";
    let expected_x = [0.0, 187.5, 375.0];
    let expected_y = [0.0, 150.0];
    let expected_z = [300.0, 300.0 + 200.0 / 3.0, 300.0 + 400.0 / 3.0, 500.0];
    let mut checked = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        let Some(summary) = store.resources().iter().find(|s| s.uuid == LGR).cloned() else {
            continue;
        };
        let object = store.get_object(&summary.uri).expect("the local grid reads");
        let found = geometry::of(object.as_reflect(), &store)
            .unwrap_or_else(|e| panic!("{}: the local grid did not evaluate: {e}", path.display()));

        let points: Vec<_> = found.patches.iter().flat_map(|p| &p.points).collect();
        assert_eq!(
            points.len(),
            expected_x.len() * expected_y.len() * expected_z.len(),
            "{}: a 2×1×3 child has 3×2×4 nodes",
            path.display()
        );
        // Node order is the one an IJK grid always uses: k slowest, then j,
        // then i. Asserting it here is what makes the cell indices below mean
        // anything.
        let mut index = 0usize;
        for z in expected_z {
            for y in expected_y {
                for x in expected_x {
                    let point = points[index];
                    for (axis, want) in [x, y, z].into_iter().enumerate() {
                        assert!(
                            (point[axis] - want).abs() < 1e-9,
                            "{}: node {index} is {point:?}, expected ({x}, {y}, {z})",
                            path.display()
                        );
                    }
                    index += 1;
                    checked += 1;
                }
            }
        }

        // The parent grid must be untouched by the child having been read -
        // the window read is a slice of its array, not a mutation of it.
        let parent = store
            .resources()
            .iter()
            .find(|s| s.uuid == "df2103a0-fa3d-11e5-b8d4-0002a5d5c51b")
            .cloned()
            .expect("the corpus holds the parent grid");
        let parent = store.get_object(&parent.uri).expect("the parent reads");
        let parent = geometry::of(parent.as_reflect(), &store).expect("the parent evaluates");
        assert_eq!(
            parent.point_count(),
            16,
            "{}: the parent still holds its 2 levels × 8 lines",
            path.display()
        );

        // The CRS is not on the geometry but on the scene object, and it is the
        // parent's: a local grid names none of its own, so without the hop
        // through the parent window it would export unplaceable.
        let crs = energyml_domain::crs::of(object.as_reflect(), &store)
            .expect("the CRS resolves")
            .expect("a local grid is in its parent's system, not in none");
        assert_eq!(
            crs.projected.epsg(),
            Some(23031),
            "{}: the local grid should inherit the parent's EPSG",
            path.display()
        );
    }
    assert!(checked > 0, "the corpus holds `{LGR}`; it was not evaluated");
    eprintln!("{checked} local-grid node(s) checked against arithmetic");
}

/// A frame's nodes land on the line its trajectory draws.
///
/// The pairing that has to hold for a log to be readable: a
/// `WellboreFrameRepresentation` says "at measured depth 400", and the point
/// it comes out at has to be the point the well path passes through at 400.
/// Until 01/08 this could not be checked at all - a frame whose trajectory
/// carried a `Geometry` was refused by name, so the six frames in the corpus
/// produced nothing.
///
/// Checked against arithmetic rather than against the same code path: fesapi's
/// `acd2cdcf` is a vertical line at x = 275, y = 75 whose parameters run
/// 0…1000, so every node of every frame on it is `(275, 75, md)` - a number
/// this test knows without asking the library.
#[test]
#[cfg(feature = "hdf5")]
fn a_frame_lands_on_the_line_its_trajectory_draws() {
    let files = corpus();
    if skip(&files, "a_frame_lands_on_the_line_its_trajectory_draws") {
        return;
    }
    const TRAJECTORY: &str = "acd2cdcf-bb5d-48da-bd0e-9aeff3e52180";
    let mut checked = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        let has_trajectory = store.resources().iter().any(|s| s.uuid == TRAJECTORY);
        if !has_trajectory {
            continue;
        }
        for summary in store.resources() {
            if !summary.qualified_type.contains("WellboreFrame") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let found = match geometry::of(object.as_reflect(), &store) {
                Ok(found) => found,
                // A seismic frame keeps its depths in time, not in metres, and
                // is refused elsewhere for that reason.
                Err(e) => {
                    eprintln!("  {} in {}: {e}", summary.uuid, path.display());
                    continue;
                }
            };
            for point in found.patches.iter().flat_map(|patch| &patch.points) {
                assert!(
                    (point[0] - 275.0).abs() < 1e-6 && (point[1] - 75.0).abs() < 1e-6,
                    "{}: {} put a node at {point:?}, off a line that never leaves (275, 75)",
                    path.display(),
                    summary.uuid
                );
                assert!(
                    (0.0..=1000.0).contains(&point[2]),
                    "{}: {} put a node at depth {} - the line runs 0 to 1000",
                    path.display(),
                    summary.uuid,
                    point[2]
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked > 0,
        "the corpus has frames on `{TRAJECTORY}`; none was placed"
    );
    eprintln!("{checked} frame node(s) placed on their trajectory");
}

/// Parametric geometry: points that live on lines.
///
/// Two families depended on this one form - the IJK grids whose pillars are
/// parametric, and every wellbore trajectory - so the test checks both come
/// out, and that what is refused is refused *by name* rather than by silence.
#[test]
#[cfg(feature = "hdf5")]
fn parametric_geometry_yields_points_on_its_lines() {
    let files = corpus();
    if skip(&files, "parametric_geometry_yields_points_on_its_lines") {
        return;
    }
    let mut grids = 0usize;
    let mut trajectories = 0usize;
    let mut frames = 0usize;
    let mut refused: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    // The grid that made the kind table worth getting right: six pillars
    // declared 1, 4, 0, 0, 0, 0 - a linear spline, a Z linear cubic and four
    // vertical lines. It shipped in three of the corpus's packages and was
    // refused by all of them until the splines were evaluated.
    const CURVED: &str = "3ce91933-4f6f-4f35-b0ac-4ba4672f0a87";
    let mut curved = 0usize;
    // How many packages **hold** the curved-pillar grid, counted apart from how
    // many evaluated it. Pinning the second alone was pinning a measurement of
    // the corpus: the number moved the day `testdata/` was restored from other
    // copies of the same packages, and the test then reported a defect about a
    // reader that had not changed. What it means to say is that the two agree.
    let mut ships = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            if summary.uuid == CURVED {
                ships += 1;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            if !geometry::is_supported(object.as_reflect()) {
                continue;
            }
            let parametric = summary.qualified_type.contains("WellboreTrajectory");
            match geometry::of(object.as_reflect(), &store) {
                Ok(found) => {
                    if parametric {
                        // A trajectory is its survey stations, joined. Two
                        // stations is the least that is a path rather than a
                        // point.
                        assert!(
                            found.point_count() >= 2,
                            "{}: {} has {} station(s)",
                            path.display(),
                            summary.uuid,
                            found.point_count()
                        );
                        assert!(matches!(found.patches[0].topology, Topology::Polylines(_)));
                        trajectories += 1;
                    }
                    for patch in &found.patches {
                        // Same rule as `every_supported_representation_yields_points`:
                        // non-finite values occur in real files as "absent"
                        // sentinels (2 of 510 points in ALWYN 9bca359d), so the
                        // guard is against a patch that is *entirely*
                        // non-finite - the shape of an evaluation bug.
                        let finite = patch
                            .points
                            .iter()
                            .filter(|p| p.iter().all(|c| c.is_finite()))
                            .count();
                        assert!(
                            // Same exemption, same reason: a selection
                            // evaluates nothing, so all-non-finite is the
                            // support's no-data area and not our arithmetic.
                            patch.points.is_empty()
                                || finite > 0
                                || selects_rather_than_computes(&summary.qualified_type),
                            "{}: {} evaluated to nothing but non-finite points",
                            path.display(),
                            summary.uuid
                        );
                    }
                    if summary.qualified_type.contains("IjkGrid")
                        && matches!(found.patches[0].topology, Topology::Hexahedra { .. })
                    {
                        grids += 1;
                        if summary.uuid == CURVED {
                            curved += 1;
                        }
                    }
                    if summary.qualified_type.contains("WellboreFrame") {
                        // A frame is where the logs hang. Its nodes are its
                        // measured depths placed on the trajectory - which,
                        // when the trajectory has a parametric line of its
                        // own, needs that line evaluated.
                        assert!(
                            found.point_count() >= 2,
                            "{}: {} has {} node(s)",
                            path.display(),
                            summary.uuid,
                            found.point_count()
                        );
                        frames += 1;
                    }
                }
                Err(energyml_geometry::Error::Unsupported { form, .. }) => {
                    refused.insert(form.to_string());
                }
                Err(e) if e.is_missing_file() => {}
                // A plane with no extent: named, and about the file.
                Err(energyml_geometry::Error::Unbounded { form, .. }) => {
                    refused.insert(form.to_string());
                }
                Err(energyml_geometry::Error::Malformed { .. })
                    if !path.to_string_lossy().contains("testingPackageCpp") =>
                {
                    // Defective real files refuse by name; the fesapi packages
                    // are conformant, so a malformed refusal there stays fatal.
                    eprintln!("  malformed, refused by name: {}", summary.uuid);
                }
                Err(e) => panic!("{}: {} - {e}", path.display(), summary.uuid),
            }
        }
    }

    eprintln!(
        "{trajectories} wellbore trajectory/ies, {frames} frame(s), {grids} IJK grid(s), \
         {curved} of them with a curved pillar"
    );
    if !refused.is_empty() {
        eprintln!("refused by name: {refused:?}");
    }
    assert!(
        trajectories > 0,
        "the corpus has 9 wellbore trajectories; none was evaluated"
    );
    assert!(
        frames > 0,
        "the corpus has frames on a trajectory that carries a parametric line; none came out"
    );
    // Until 01/08 this asserted the opposite - that a spline was *refused* by
    // name. Now that the four kinds are evaluated, the thing worth pinning is
    // that no line kind is left behind, and that the grid which actually uses
    // a curved one comes out rather than merely not being mentioned.
    assert!(ships > 0, "the Z linear cubic grid is in no package here");
    assert_eq!(
        curved, ships,
        "the Z linear cubic grid is in {ships} package(s) and {curved} evaluated          with a curved pillar"
    );
    assert!(
        !refused.iter().any(|r| r.contains("spline")),
        "a parametric line kind is still refused: {refused:?}"
    );
}

#[test]
fn a_property_is_attached_only_where_it_lines_up() {
    // The rule the whole attribute path exists for: values go on the elements
    // they belong to, or they do not go at all. The corpus is the test - it
    // contains a per-cell property that fits (`cellIndex` on a 2-cell grid),
    // one that does not (a time series storing every step in one dataset), and
    // one that hangs on the representation as a whole.
    let files = corpus();
    if skip(&files, "a_property_is_attached_only_where_it_lines_up") {
        return;
    }
    let registry = registry();
    let mut attached = 0usize;
    let mut refused = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        let properties: Vec<Box<dyn energyml_core::registry::DynObject>> = store
            .resources()
            .iter()
            .filter(|s| s.qualified_type.ends_with("Property"))
            .filter_map(|s| store.get_object(&s.uri).ok())
            .collect();
        let reflected: Vec<&dyn energyml_core::value::Reflect> =
            properties.iter().map(|o| o.as_reflect()).collect();

        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            if !energyml_geometry::geometry::is_supported(object.as_reflect()) {
                continue;
            }
            let Ok(built) = energyml_geometry::scene::object_of(object.as_reflect(), &store, &summary.uri)
            else {
                continue;
            };
            let (found, notes) = energyml_geometry::scene::attributes_for(&built, &reflected, &store);

            for attribute in &found {
                // The invariant: exactly one value per element, times the
                // number of components.
                let elements = match attribute.scope {
                    energyml_geometry::AttributeScope::Points => built.geometry.point_count(),
                    energyml_geometry::AttributeScope::Cells => built.geometry.cell_count(),
                };
                assert_eq!(
                    attribute.values.len(),
                    elements * attribute.components as usize,
                    "{}: `{}` is attached but does not line up",
                    path.display(),
                    attribute.name
                );
                attached += 1;
            }
            for note in &notes {
                // Every refusal names the property and says why.
                assert!(note.len() > 30, "a refusal that says nothing: {note}");
                refused += 1;
            }
        }
    }
    eprintln!("{attached} attribute(s) attached, {refused} refusal(s) explained");
}

#[test]
fn the_node_count_of_an_ijk_grid_agrees_with_the_points_it_actually_has() {
    // Two independent paths to the same number, which is the only way to trust
    // either: `energyml_domain::property::element_count` derives it from
    // `Ni × Nj × Nk` plus the declared split coordinate lines, while the
    // geometry reader gets it by reading the points. A grid with a fault has
    // **more** nodes than `(ni+1)(nj+1)(nk+1)` - one extra line per split -
    // and this is where a formula that ignores splits shows up.
    use energyml_domain::property::{IndexableElement, element_count};

    let files = corpus();
    if skip(
        &files,
        "the_node_count_of_an_ijk_grid_agrees_with_the_points_it_actually_has",
    ) {
        return;
    }
    let registry = registry();
    let mut found = 0usize;
    let mut checked = 0usize;
    let mut with_faults = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for summary in store.resources() {
            if !summary.qualified_type.contains("IjkGridRepresentation") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            // Only the grids whose geometry this build can evaluate: the rest
            // refuse by name elsewhere, and their point count does not exist.
            found += 1;
            let Ok(built) = geometry::of(object.as_reflect(), &store) else {
                // Without an array backend no grid geometry exists to compare
                // against; that is a build without `--features hdf5`, not a
                // failure of the count.
                continue;
            };
            let Ok(declared) = element_count(object.as_reflect(), IndexableElement::Nodes) else {
                continue;
            };
            assert_eq!(
                declared as usize,
                built.point_count(),
                "{}: {} says {declared} nodes and its geometry holds {}",
                path.display(),
                summary.uuid,
                built.point_count()
            );
            checked += 1;

            // And the faulted case is really in the corpus, or this test is
            // only checking the easy half: a grid with splits has strictly
            // more coordinate lines than its pillar lattice.
            let lines = element_count(object.as_reflect(), IndexableElement::CoordinateLines)
                .expect("a grid has coordinate lines");
            let pillars =
                element_count(object.as_reflect(), IndexableElement::Pillars).expect("a grid has pillars");
            if lines > pillars {
                with_faults += 1;
            }
        }
    }
    if checked == 0 {
        eprintln!(
            "skipped: {found} IJK grid(s) found and none evaluated - this build has no array \
             backend (`--features hdf5`), so there are no points to compare the count with"
        );
        return;
    }
    assert!(
        with_faults > 0,
        "no grid in the corpus has a split coordinate line - the formula that ignores them would \
         pass this test unchanged"
    );
    eprintln!("{checked} IJK grid(s) cross-checked, {with_faults} of them with split coordinate lines");
}

/// A vertical pillar is vertical all the way down.
///
/// The invariant a faulted **parametric** grid broke, and the one that says
/// whether the two halves of the geometry agree. `PillarShape = vertical` means
/// every coordinate line is a straight vertical line, so the two ends of a
/// cell's vertical edge - corner `c` and corner `c + 4` - are the *same
/// coordinate line at two levels* and must share x and y exactly.
///
/// Both halves of this were tested already, and separately: a fault test on the
/// grid with explicit points, a parametric test on grids with no faults. The
/// combination - a fault *and* parametric points, which is `37c45c00`, four
/// faulted sugar cubes - was covered by neither, and it was the one that was
/// wrong. A grid with 6 pillars and 2 split lines has 8 coordinate lines and 6
/// parametric lines; the evaluation used 6 for both, so every level after the
/// first took its x and y from the wrong pillar. The point count was right, the
/// depths were right, and the cells came out sheared into bowties.
#[test]
#[cfg(feature = "hdf5")]
fn a_vertical_pillar_stays_over_itself() {
    let files = corpus();
    if skip(&files, "a_vertical_pillar_stays_over_itself") {
        return;
    }
    let mut grids = 0usize;
    let mut faulted = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            if !summary.qualified_type.contains("IjkGrid") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let reflect = object.as_reflect();
            // Only the grids that claim it. A curved pillar is free to move.
            let vertical = reflect
                .field("Geometry")
                .and_then(|g| g.as_object().map(|g| g.field("PillarShape")))
                .flatten()
                .and_then(|shape| shape.as_scalar().and_then(|s| s.as_str().map(str::to_string)))
                .is_some_and(|shape| shape == "vertical");
            if !vertical {
                continue;
            }
            // Whether this grid is faulted, asked of the file and not of the
            // result: the whole point is that the result looked fine.
            let split = reflect
                .field("Geometry")
                .and_then(|g| g.as_object())
                .map(|g| {
                    ["SplitCoordinateLines", "ColumnLayerSplitCoordinateLines"]
                        .iter()
                        .any(|name| g.field(name).is_some_and(|f| !f.is_empty()))
                })
                .unwrap_or(false);

            let Ok(found) = geometry::of(reflect, &store) else {
                continue;
            };
            for patch in &found.patches {
                let Topology::Hexahedra { cells, .. } = &patch.topology else {
                    continue;
                };
                if cells.is_empty() {
                    continue;
                }
                grids += 1;
                if split {
                    faulted += 1;
                }
                for (index, cell) in cells.iter().enumerate() {
                    for corner in 0..4 {
                        let top = patch.points[cell[corner] as usize];
                        let bottom = patch.points[cell[corner + 4] as usize];
                        assert!(
                            (top[0] - bottom[0]).abs() < 1e-6 && (top[1] - bottom[1]).abs() < 1e-6,
                            "{}: {} cell {index} edge {corner} runs from ({}, {}) to ({}, {}) \
                             on a grid whose pillars are declared vertical - the two ends are on \
                             different coordinate lines",
                            path.display(),
                            summary.uuid,
                            top[0],
                            top[1],
                            bottom[0],
                            bottom[1]
                        );
                    }
                }
            }
        }
    }

    assert!(grids > 0, "no grid in the corpus declares vertical pillars");
    assert!(
        faulted > 0,
        "no faulted grid among them - the reading that ignores split coordinate lines would \
         pass this test unchanged"
    );
    eprintln!("{grids} vertical-pillar grid patch(es) checked, {faulted} of them faulted");
}

/// A 2D point array holds **two** coordinates per point.
///
/// `Point2dExternalArray` ends in `ExternalArray` like every 3D array, and read
/// three at a time it does not fail: it yields two thirds as many points, each
/// built from one point's x and y and the *next* point's x, with the following
/// y taken for a depth. Every number is real, in the right unit, in file order.
/// The corpus's geobody boundary holds a (6, 2) dataset and came out as 4
/// points scattered where nothing is.
#[test]
#[cfg(feature = "hdf5")]
fn a_two_dimensional_point_array_is_read_two_at_a_time() {
    let files = corpus();
    if skip(&files, "a_two_dimensional_point_array_is_read_two_at_a_time") {
        return;
    }
    let mut checked = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let reflect = object.as_reflect();
            if !geometry::is_supported(reflect) {
                continue;
            }
            let Some(array) = find_2d(reflect) else { continue };
            // What the file says is there, before anything reads it.
            let declared =
                energyml_domain::values::count(array, &store, "2d").expect("a 2D array declares its length");
            let Ok(found) = geometry::of(reflect, &store) else {
                continue;
            };
            let points: usize = found.patches.iter().map(|p| p.points.len()).sum();
            assert_eq!(
                points,
                declared / 2,
                "{}: {} declares {declared} coordinate(s) in a 2D array and yielded {points} \
                 point(s) - a 2D array read three at a time loses a third of them and moves \
                 the rest",
                path.display(),
                summary.uuid
            );
            // z is not invented: the array says nothing about depth.
            for patch in &found.patches {
                for point in &patch.points {
                    assert_eq!(
                        point[2].abs(),
                        0.0,
                        "{}: a 2D point acquired a depth",
                        summary.uuid
                    );
                }
            }
            checked += 1;
        }
    }

    assert!(
        checked > 0,
        "no 2D point array in the corpus - this test proves nothing"
    );
    eprintln!("{checked} representation(s) with a 2D point array");
}

/// The first `Point2d…Array` under an object, at any depth.
#[cfg(feature = "hdf5")]
fn find_2d(object: &dyn energyml_core::value::Reflect) -> Option<&dyn energyml_core::value::Reflect> {
    if object.type_info().name.starts_with("Point2d") {
        return Some(object);
    }
    for field in object.fields() {
        match object.field(field.name) {
            Some(energyml_core::value::FieldValue::Object(nested)) => {
                if let Some(found) = find_2d(nested) {
                    return Some(found);
                }
            }
            Some(energyml_core::value::FieldValue::List(items)) => {
                for item in items {
                    if let energyml_core::value::FieldValue::Object(nested) = item
                        && let Some(found) = find_2d(nested)
                    {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// A borrowed lattice lands on its support's own nodes.
///
/// `Point3dFromRepresentationLatticeArray` names a regular selection of another
/// representation's nodes, and the selection reads two ways - a step along an
/// axis of the support, or a step in the support's flat node order. Both are in
/// range on every file in the corpus and both produce the declared number of
/// points, so nothing about the *count* separates them. Only the positions do.
///
/// The corpus's two producers both encode the simplest possible selection:
/// start at 0, step 1 along every axis, over a support of exactly the derived
/// representation's shape. That selection can only mean **every node of the
/// support, once** - so the check is independent of how this crate indexes: the
/// derived points must be the support's points, as a set, with nothing repeated
/// and nothing left out.
///
/// Read the other way, ALWYN's 987 387 slots land on 2 011 distinct positions -
/// a thin diagonal band with the horizon's depths stacked on it. That fails
/// here at the first assertion, which is the point.
#[test]
#[cfg(feature = "hdf5")]
fn a_borrowed_lattice_lands_on_its_supports_nodes() {
    use std::collections::BTreeSet;

    let files = corpus();
    if skip(&files, "a_borrowed_lattice_lands_on_its_supports_nodes") {
        return;
    }
    let mut checked = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let Some(borrowed) = find_borrowed(object.as_reflect()) else {
                continue;
            };

            // Read the selection from the **file**, not from the reader whose
            // answer is under test.
            let Some(lattice) = field_of(borrowed, "NodeIndicesOnSupportingRepresentation") else {
                continue;
            };
            let start = int_of(lattice, "StartValue").unwrap_or(0);
            let offsets = offsets_of(lattice);
            let identity = start == 0 && !offsets.is_empty() && offsets.iter().all(|(value, _)| *value == 1);
            if !identity {
                eprintln!(
                    "{}: {} selects with start {start} and steps {:?} - not the identity case \
                     this test can judge without re-deriving the indices",
                    path.display(),
                    summary.uuid,
                    offsets.iter().map(|(v, _)| *v).collect::<Vec<_>>()
                );
                continue;
            }

            let Some(uuid) = reference_uuid(borrowed, "SupportingRepresentation") else {
                continue;
            };
            let Some(support) = energyml_domain::source::ObjectSource::object(&store, &uuid) else {
                continue;
            };
            let Ok(support_geometry) = geometry::of(support.as_reflect(), &store) else {
                continue;
            };
            let (support_points, _) = support_geometry.merged();

            // The selection covers the whole support, or the identity reading
            // is not what this file means either.
            let selected: usize = offsets.iter().map(|(_, count)| *count as usize + 1).product();
            if selected != support_points.len() {
                eprintln!(
                    "{}: {} selects {selected} of the support's {} node(s) - a real subset, \
                     which this test does not judge",
                    path.display(),
                    summary.uuid,
                    support_points.len()
                );
                continue;
            }

            let derived = points::read(
                borrowed,
                &store,
                &format!("{}.SupportingGeometry", summary.uuid),
                Vertical::Down,
            )
            .expect("a borrowed lattice evaluates");

            assert_eq!(
                derived.len(),
                support_points.len(),
                "{}: {} borrowed {} point(s) from a support of {}",
                path.display(),
                summary.uuid,
                derived.len(),
                support_points.len()
            );

            let key = |p: &[f64; 3]| {
                (
                    (p[0] * 1e6).round() as i64,
                    (p[1] * 1e6).round() as i64,
                    (p[2] * 1e6).round() as i64,
                )
            };
            let taken: BTreeSet<_> = derived.iter().map(key).collect();
            let available: BTreeSet<_> = support_points.iter().map(key).collect();

            assert_eq!(
                taken.len(),
                derived.len(),
                "{}: {} borrowed {} point(s) that land on only {} distinct position(s) - \
                 the selection is being read as a step in the flat node order, and the \
                 representation is a band rather than a map",
                path.display(),
                summary.uuid,
                derived.len(),
                taken.len()
            );
            assert_eq!(
                taken,
                available,
                "{}: {} does not land on its support's nodes",
                path.display(),
                summary.uuid
            );
            eprintln!(
                "{}: {} borrows all {} node(s) of {uuid}",
                path.display(),
                summary.uuid,
                derived.len()
            );
            checked += 1;
        }
    }

    assert!(
        checked > 0,
        "no borrowed lattice in the corpus was judged; the reading this test exists to pin \
         is untested"
    );
    eprintln!("{checked} borrowed lattice(s) checked");
}

/// The first `Point3dFromRepresentationLatticeArray` under an object.
#[cfg(feature = "hdf5")]
fn find_borrowed(object: &dyn energyml_core::value::Reflect) -> Option<&dyn energyml_core::value::Reflect> {
    if object.type_info().name == "Point3dFromRepresentationLatticeArray" {
        return Some(object);
    }
    for field in object.fields() {
        match object.field(field.name) {
            Some(energyml_core::value::FieldValue::Object(child)) => {
                if let Some(found) = find_borrowed(child) {
                    return Some(found);
                }
            }
            Some(energyml_core::value::FieldValue::List(items)) => {
                for item in items {
                    if let energyml_core::value::FieldValue::Object(child) = item
                        && let Some(found) = find_borrowed(child)
                    {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(feature = "hdf5")]
fn field_of<'a>(
    object: &'a dyn energyml_core::value::Reflect,
    name: &str,
) -> Option<&'a dyn energyml_core::value::Reflect> {
    match object.field(name)? {
        energyml_core::value::FieldValue::Object(child) => Some(child),
        _ => None,
    }
}

/// `(value, count)` per axis, straight out of the document.
#[cfg(feature = "hdf5")]
fn offsets_of(lattice: &dyn energyml_core::value::Reflect) -> Vec<(i64, i64)> {
    let items = match lattice.field("Offset") {
        Some(energyml_core::value::FieldValue::List(list)) => {
            list.iter().filter_map(|i| i.as_object()).collect::<Vec<_>>()
        }
        Some(energyml_core::value::FieldValue::Object(one)) => vec![one],
        _ => Vec::new(),
    };
    items
        .into_iter()
        .map(|offset| {
            (
                int_of(offset, "Value").unwrap_or(1),
                int_of(offset, "Count").unwrap_or(0),
            )
        })
        .collect()
}

/// A whole number read straight off the object, without the crate's own
/// helpers: a test that reaches for the reader's utilities to decide what the
/// document said has stopped being independent of the reader.
#[cfg(feature = "hdf5")]
fn int_of(object: &dyn energyml_core::value::Reflect, field: &str) -> Option<i64> {
    match object.field(field)? {
        energyml_core::value::FieldValue::Scalar(value) => value.to_string().trim().parse().ok(),
        _ => None,
    }
}

#[cfg(feature = "hdf5")]
fn reference_uuid(object: &dyn energyml_core::value::Reflect, field: &str) -> Option<String> {
    let energyml_core::value::FieldValue::Object(reference) = object.field(field)? else {
        return None;
    };
    energyml_core::identity::uuid_of_reflect(reference)
        .map(str::to_string)
        .filter(|uuid| !uuid.is_empty())
}

/// A borrowed line lattice is the support's own lines.
///
/// No file in the corpus carries a `ParametricLineFromRepresentationLatticeArray`,
/// so the subject is built here - but the **expectation is not**. It comes from
/// the IJK grid `37c45c00`, whose parametric geometry the corpus validates every
/// time this suite runs.
///
/// The construction: take the grid's own parameters, and evaluate them twice.
/// Once against the grid's own `ParametricLineArray`, and once against a
/// borrowing that selects all six of those same lines out of the grid - start
/// 0, step 1 along each axis of a `(Nj+1) × (Ni+1)` pillar lattice, which is
/// what the standard says a line index means for a column-layer grid. The two
/// arrays differ in **nothing but how they name their lines**, so if the
/// borrowing is read correctly they evaluate to the same points, to the bit.
///
/// What it would catch: a start read per-axis instead of linearized, a step
/// read flat instead of per-axis, the pillar lattice taken as `Ni × Nj` instead
/// of `(Ni+1) × (Nj+1)`, or the split coordinate lines wrongly counted among
/// the parametric ones. Each of those reorders or overruns the six.
#[test]
#[cfg(feature = "hdf5")]
fn a_borrowed_line_lattice_is_the_supports_own_lines() {
    use energyml_eml_2_3::{
        ExternalDataArray, ExternalDataArrayPart, FloatingPointExternalArray, FloatingPointType,
        IntegerConstantArray, IntegerLatticeArray,
    };
    use energyml_resqml_2_2::AbstractValueArray;
    use energyml_resqml_2_2::geometry::{
        AbstractParametricLineArray, AbstractPoint3dArray, ParametricLineArray,
        ParametricLineFromRepresentationLatticeArray, Point3dExternalArray, Point3dParametricArray,
    };

    const GRID: &str = "37c45c00-fa3e-11e5-a21e-0002a5d5c51b";
    let Some(path) = crate::fixtures::find("testingPackageCpp22.epc") else {
        eprintln!("skipped: the reference package is not in the fixtures");
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");

    // The grid's own arrays, transcribed from the part. Ni = 2, Nj = 1, so the
    // pillar lattice is 2 × 3 - six lines - and the parameters are 24: three
    // levels over eight coordinate lines, six pillars and two splits.
    let part = |name: &str, count: i64| ExternalDataArray {
        external_data_array_part: vec![ExternalDataArrayPart {
            count: vec![count],
            path_in_external_file: format!("/resqml22/{GRID}/{name}"),
            start_index: vec![0],
            uri: "testingPackageCpp22.h5".into(),
            mime_type: Some("application/x-hdf5".into()),
        }],
    };
    let parameters = || {
        Some(AbstractValueArray::FloatingPointExternalArray(
            FloatingPointExternalArray {
                array_floating_point_type: FloatingPointType::ArrayOfFloat32LE,
                count_per_value: 1,
                values: part("PointParameters", 24),
                ..Default::default()
            },
        ))
    };
    let own_lines = || ParametricLineArray {
        control_points: Some(Box::new(AbstractPoint3dArray::Point3dExternalArray(
            Point3dExternalArray {
                coordinates: part("ControlPoints", 18),
            },
        ))),
        knot_count: 1,
        line_kind_indices: Some(energyml_eml_2_3::AbstractIntegerArray::IntegerConstantArray(
            IntegerConstantArray {
                value: 0,
                count: 6,
                statistics: Vec::new(),
            },
        )),
        ..Default::default()
    };

    let stored = Point3dParametricArray {
        parameters: parameters(),
        parametric_lines: Some(AbstractParametricLineArray::ParametricLineArray(own_lines())),
        ..Default::default()
    };
    let expected =
        points::read(&stored, &store, "stored", Vertical::Down).expect("the grid's own lines evaluate");
    assert_eq!(expected.len(), 24, "the transcription does not match the part");

    // The same parameters, over lines borrowed from the grid: every pillar,
    // once, in pillar order. `(Nj + 1) × (Ni + 1)` = 2 × 3, so the counts are
    // 1 and 2 - one increment along the slow axis, two along the fast.
    let borrowed = Point3dParametricArray {
        parameters: parameters(),
        parametric_lines: Some(
            AbstractParametricLineArray::ParametricLineFromRepresentationLatticeArray(
                ParametricLineFromRepresentationLatticeArray {
                    line_indices_on_supporting_representation: IntegerLatticeArray {
                        start_value: 0,
                        offset: vec![
                            IntegerConstantArray {
                                value: 1,
                                count: 1,
                                statistics: Vec::new(),
                            },
                            IntegerConstantArray {
                                value: 1,
                                count: 2,
                                statistics: Vec::new(),
                            },
                        ],
                        ..Default::default()
                    },
                    supporting_representation: energyml_eml_2_3::DataObjectReference {
                        uuid: GRID.into(),
                        qualified_type: "resqml22.IjkGridRepresentation".into(),
                        title: "Four faulted sugar cubes".into(),
                        ..Default::default()
                    },
                },
            ),
        ),
        ..Default::default()
    };
    let got = points::read(&borrowed, &store, "borrowed", Vertical::Down).expect("borrowed lines evaluate");

    assert_eq!(
        got, expected,
        "the same parameters over the same six lines, named two ways, gave two answers"
    );
    eprintln!("{} point(s) identical over borrowed lines", got.len());

    // And a selection that runs off the end is refused rather than wrapped.
    let mut past = borrowed;
    if let Some(AbstractParametricLineArray::ParametricLineFromRepresentationLatticeArray(l)) =
        &mut past.parametric_lines
    {
        l.line_indices_on_supporting_representation.offset[1].count = 9;
    }
    assert!(
        points::read(&past, &store, "past the end", Vertical::Down).is_err(),
        "a selection reaching past the support's lines must refuse, not wrap"
    );
}

/// The number of patch elements the document itself declares - the independent
/// source the reader is confronted with, per the cross-audit rule: at least
/// one side of every check must not be code this workspace wrote about the
/// same question.
#[cfg(feature = "hdf5")]
fn declared_patches(object: &dyn energyml_core::value::Reflect) -> usize {
    for name in ["TrianglePatch", "NodePatch", "NodePatchGeometry", "LinePatch"] {
        if let Some(field) = object.field(name) {
            let declared = field.occurrences();
            if declared > 0 {
                return declared;
            }
        }
    }
    0
}

#[test]
#[cfg(feature = "hdf5")]
fn every_declared_patch_is_read_and_none_is_lost_on_the_way_out() {
    // Asked, in so many words, by a review: that an export
    // carries **every** patch of a set representation, not just the first.
    // Two confrontations. The patch count is checked against the number of
    // patch elements the XML declares - not against the reader's own idea of
    // itself. And the drawable - what the view uploads and what OBJ and VTK
    // write - must hold the sum of the patches' points, missing only the
    // non-finite ones it counts in `dropped`: a patch read and then lost in
    // the merge would pass every per-patch assertion above it.
    let files = corpus();
    if skip(
        &files,
        "every_declared_patch_is_read_and_none_is_lost_on_the_way_out",
    ) {
        return;
    }
    let mut multi = 0usize;
    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            if !geometry::is_supported(object.as_reflect()) {
                continue;
            }
            let declared = declared_patches(object.as_reflect());
            if declared == 0 {
                continue;
            }
            // A package whose `.h5` is not in the corpus is a corpus gap, not
            // a patch defect; the sibling test already counts those.
            let Ok(geometry) = geometry::of(object.as_reflect(), &store) else {
                continue;
            };
            assert_eq!(
                geometry.patches.len(),
                declared,
                "{}: {} ({}) read {} patch(es) where the document declares {declared}",
                path.display(),
                summary.uuid,
                geometry.kind,
                geometry.patches.len(),
            );
            if declared > 1 {
                multi += 1;
            }
            let sum: usize = geometry.patches.iter().map(|patch| patch.points.len()).sum();
            let drawable = geometry.drawable();
            assert_eq!(
                drawable.points.len() + drawable.dropped,
                sum,
                "{}: {} ({}) merged {} of {sum} points",
                path.display(),
                summary.uuid,
                geometry.kind,
                drawable.points.len(),
            );
        }
    }
    // A test that never met a multi-patch representation proved nothing - the
    // first attempt at two checks in the cross-audit suite passed over empty
    // sets, and this clause is what that lesson looks like here.
    assert!(
        multi > 0,
        "no multi-patch representation in the corpus; the assertion never bit"
    );
    eprintln!("multi-patch representations checked: {multi}");
}

/// Two real surfaces, cut and filled - the test synthetic squares do not pass.
///
/// The assertions come from the **definition** of a panel, never from a file's
/// content: its vertices are on the trace, its indices are inside its own
/// patch, no coordinate is undefined, and inside a patch the thickness does not
/// change sign - which is precisely what cutting at the crossing promises. What
/// the files say (how many patches, how many crossings) is **displayed**, never
/// pinned.
#[test]
#[cfg(feature = "hdf5")]
fn a_panel_between_two_real_surfaces_stays_on_its_trace() {
    use energyml_geometry::fence;
    use energyml_geometry::section;

    /// A surface ready to cut: its uuid, its placed points, its triangles.
    type Surface = (String, Vec<[f64; 3]>, Vec<[u32; 3]>);

    let files = corpus();
    if skip(&files, "a_panel_between_two_real_surfaces_stays_on_its_trace") {
        return;
    }
    let mut panels = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        // The package's triangulated surfaces, with their placed points.
        let mut surfaces: Vec<Surface> = Vec::new();
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            if !geometry::is_supported(object.as_reflect()) {
                continue;
            }
            let Ok(geometry) = geometry::of(object.as_reflect(), &store) else {
                continue;
            };
            let mut points: Vec<[f64; 3]> = Vec::new();
            let mut triangles: Vec<[u32; 3]> = Vec::new();
            for patch in &geometry.patches {
                let base = points.len() as u32;
                if let Topology::Triangles(list) = &patch.topology {
                    triangles.extend(list.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
                    points.extend(patch.points.iter().copied());
                }
            }
            if triangles.len() >= 2 && points.iter().any(|p| p.iter().all(|v| v.is_finite())) {
                surfaces.push((summary.uuid.clone(), points, triangles));
            }
            if surfaces.len() == 2 {
                break;
            }
        }
        if surfaces.len() < 2 {
            continue;
        }

        // A trace crossing the two surfaces' common footprint, corner to
        // corner: it meets material if there is any.
        let finite = |points: &[[f64; 3]], axis: usize, low: bool| -> f64 {
            points.iter().map(|p| p[axis]).filter(|v| v.is_finite()).fold(
                if low { f64::MAX } else { f64::MIN },
                if low { f64::min } else { f64::max },
            )
        };
        let x0 = finite(&surfaces[0].1, 0, true).max(finite(&surfaces[1].1, 0, true));
        let x1 = finite(&surfaces[0].1, 0, false).min(finite(&surfaces[1].1, 0, false));
        let y0 = finite(&surfaces[0].1, 1, true).max(finite(&surfaces[1].1, 1, true));
        let y1 = finite(&surfaces[0].1, 1, false).min(finite(&surfaces[1].1, 1, false));
        if !(x1 > x0 && y1 > y0) {
            continue;
        }
        let mid = (y0 + y1) / 2.0;
        let trace = vec![[x0, mid], [x1, mid]];

        let upper = fence::curve(&section::cut(&surfaces[0].1, &surfaces[0].2, &trace));
        let lower = fence::curve(&section::cut(&surfaces[1].1, &surfaces[1].2, &trace));
        let panel = fence::panel(&upper, &lower, &trace);
        if panel.is_empty() {
            continue;
        }
        panels += 1;
        eprintln!(
            "{}: {} × {} → {} patch(es), {} note(s)",
            path.file_name().unwrap_or_default().to_string_lossy(),
            surfaces[0].0,
            surfaces[1].0,
            panel.patches.len(),
            panel.notes.len(),
        );
        for note in &panel.notes {
            eprintln!("    {note}");
        }

        for patch in &panel.patches {
            assert!(
                !patch.triangles.is_empty(),
                "a patch with no triangle is not material"
            );
            for point in &patch.points {
                assert!(point.iter().all(|v| v.is_finite()), "{point:?} is not finite");
                // The trace is horizontal here: every vertex must be on it.
                assert!((point[1] - mid).abs() < 1e-6, "{point:?} is not on the trace");
                assert!(
                    point[0] >= x0 - 1e-6 && point[0] <= x1 + 1e-6,
                    "{point:?} leaves the trace"
                );
            }
            for triangle in &patch.triangles {
                for index in triangle {
                    assert!(
                        (*index as usize) < patch.points.len(),
                        "index {index} outside the patch ({} points)",
                        patch.points.len()
                    );
                }
            }
            // The sign is constant inside a patch: that is the whole promise
            // of cutting at the crossing. The wedge (zero thickness) is allowed
            // on both sides.
            let mut seen: Option<bool> = None;
            for pair in patch.profile.chunks(2) {
                let thickness = pair[0][1] - pair[1][1];
                if thickness.abs() < 1e-9 {
                    continue;
                }
                let side = thickness > 0.0;
                match seen {
                    None => seen = Some(side),
                    Some(was) => assert_eq!(
                        was, side,
                        "a patch changes sign in its middle - the cut at the crossing did not happen"
                    ),
                }
            }
        }
    }
    // A count of zero is not a failure - no package of the corpus necessarily
    // holds two overlapping surfaces - but it must be **seen**: a test that
    // built nothing is the same colour as a test that checked everything, and
    // that is the only way it can lie.
    if panels == 0 {
        eprintln!(
            "no panel built: this test proved nothing on this corpus (no pair of triangulated surfaces overlaps)"
        );
    } else {
        eprintln!("{panels} panel(s) built on the corpus");
    }
}

/// One line borrowed from another representation - the singular of the lattice.
///
/// `ParametricLineFromRepresentationGeometry` was refused by name until now, and
/// no package here has one, so this is written against the standard and checked
/// against the support itself: **the grid's line 0, borrowed, must evaluate to
/// exactly what the grid's own array gives for line 0.** A borrowing that
/// answered anything else would be a well drawn along the wrong pillar, which
/// looks like a well.
///
/// The trajectory carries it as its `Geometry`, which is where the singular form
/// actually appears - the plural lives in a `Point3dParametricArray`.
#[test]
#[cfg(feature = "hdf5")]
fn a_borrowed_single_line_is_the_supports_own_line() {
    use energyml_eml_2_3::{
        ExternalDataArray, ExternalDataArrayPart, FloatingPointExternalArray, FloatingPointType,
        IntegerConstantArray,
    };
    use energyml_resqml_2_2::AbstractValueArray;
    use energyml_resqml_2_2::geometry::{
        AbstractParametricLineArray, AbstractParametricLineGeometry, AbstractPoint3dArray,
        ParametricLineArray, ParametricLineFromRepresentationGeometry, Point3dExternalArray,
        Point3dParametricArray,
    };

    const GRID: &str = "37c45c00-fa3e-11e5-a21e-0002a5d5c51b";
    let Some(path) = crate::fixtures::find("testingPackageCpp22.epc") else {
        eprintln!("skipped: the reference package is not in the fixtures");
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");

    let part = |name: &str, count: i64| ExternalDataArray {
        external_data_array_part: vec![ExternalDataArrayPart {
            count: vec![count],
            path_in_external_file: format!("/resqml22/{GRID}/{name}"),
            start_index: vec![0],
            uri: "testingPackageCpp22.h5".into(),
            mime_type: Some("application/x-hdf5".into()),
        }],
    };
    let own_lines = || ParametricLineArray {
        control_points: Some(Box::new(AbstractPoint3dArray::Point3dExternalArray(
            Point3dExternalArray {
                coordinates: part("ControlPoints", 18),
            },
        ))),
        knot_count: 1,
        line_kind_indices: Some(energyml_eml_2_3::AbstractIntegerArray::IntegerConstantArray(
            IntegerConstantArray {
                value: 0,
                count: 6,
                statistics: Vec::new(),
            },
        )),
        ..Default::default()
    };

    // The support's own six lines, evaluated at one parameter each: what line 0
    // is, read the ordinary way.
    let stored = Point3dParametricArray {
        parameters: Some(AbstractValueArray::FloatingPointExternalArray(
            FloatingPointExternalArray {
                array_floating_point_type: FloatingPointType::ArrayOfFloat32LE,
                count_per_value: 1,
                values: part("PointParameters", 24),
                ..Default::default()
            },
        )),
        parametric_lines: Some(AbstractParametricLineArray::ParametricLineArray(own_lines())),
        ..Default::default()
    };
    let every = points::read(&stored, &store, "stored", Vertical::Down).expect("the grid's lines evaluate");
    assert_eq!(every.len(), 24, "the transcription does not match the part");

    let borrow = |line: i64| energyml_resqml_2_2::WellboreTrajectoryRepresentation {
        uuid: "77777777-7777-7777-7777-777777777777".into(),
        geometry: Some(
            AbstractParametricLineGeometry::ParametricLineFromRepresentationGeometry(
                ParametricLineFromRepresentationGeometry {
                    line_index_on_supporting_representation: line,
                    supporting_representation: energyml_eml_2_3::DataObjectReference {
                        uuid: GRID.into(),
                        qualified_type: "resqml22.IjkGridRepresentation".into(),
                        title: "Four faulted sugar cubes".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
        ),
        ..Default::default()
    };

    let drawn = geometry::of(&borrow(0), &store).expect("the borrowed line reads");
    let got = &drawn.patches[0].points;
    assert!(!got.is_empty(), "a borrowed line with no point");
    // Line 0's control points are the first of the six, and with one knot the
    // line is its control points: the support's own reading must agree.
    for (at, point) in got.iter().enumerate() {
        let mine = every[at * 6];
        for axis in 0..3 {
            assert!(
                (point[axis] - mine[axis]).abs() < 1e-6,
                "point {at} axis {axis}: borrowed {:?} against the support's own {mine:?}",
                point
            );
        }
    }

    // And a line the support does not have is refused **by name and with the
    // count**, never folded back into range: asking for line 12 of a six-line
    // support says something about the file.
    let error = geometry::of(&borrow(12), &store).unwrap_err().to_string();
    assert!(error.contains("line 12"), "{error}");
    assert!(error.contains("which lends 6"), "{error}");
}

/// What the document states about its own size, against what the reader places.
///
/// The other half of `tests/declared.rs`. That file pins the arithmetic on
/// objects built by hand; this one confronts it with files real producers wrote,
/// which is the only place a stated count and a real one can be seen to
/// disagree.
///
/// **The shape of the assertion comes from the spec, the numbers from the
/// files.** RESQML states a node count as *the* number of nodes - a
/// `TrianglePatch`'s `NodeCount`, a `NodePatch`'s `Count`, a lattice's two axes -
/// so a reader placing **more** nodes than the document declares has read
/// something the document does not describe. The other direction is ordinary
/// and named: a lattice states its whole grid and the reader trims the row and
/// column one writer sizes in points instead of intervals (see `grid_2d`), so
/// fewer is a possibility the spec leaves open and more is not.
///
/// What it takes to make this fail: a `declared_nodes` that sums the wrong
/// field, that adds a patch twice, or that answers for a concept whose reader
/// derives its nodes rather than reading them. The `exact` counter is why an
/// always-`None` version cannot pass it quietly.
#[test]
#[cfg(feature = "hdf5")]
fn no_reader_places_more_nodes_than_its_document_states() {
    let files = corpus();
    if skip(&files, "no_reader_places_more_nodes_than_its_document_states") {
        return;
    }
    let mut exact = 0usize;
    let mut fewer: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut stated: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let Some(declared) = geometry::declared_nodes(object.as_reflect()) else {
                continue;
            };
            *stated.entry(summary.qualified_type.clone()).or_default() += 1;
            // A representation that will not evaluate says nothing about the
            // count it states: this test is about the two agreeing, not about
            // whether the geometry reads.
            let Ok(drawn) = geometry::of(object.as_reflect(), &store) else {
                continue;
            };
            let measured = drawn.point_count() as u64;
            assert!(
                measured <= declared,
                "{}: {} ({}) states {declared} node(s) and the reader placed {measured}",
                path.display(),
                summary.uuid,
                summary.qualified_type
            );
            if measured == declared {
                exact += 1;
            } else {
                *fewer.entry(summary.qualified_type.clone()).or_default() += 1;
            }
        }
    }

    for (kind, count) in &stated {
        eprintln!("{count} × {kind} state a node count");
    }
    for (kind, count) in &fewer {
        eprintln!("{count} × {kind} placed fewer nodes than stated");
    }
    assert!(
        exact > 0,
        "no representation of the corpus placed exactly what it states - either \
         `declared_nodes` answers nothing, or it answers about something else"
    );
    eprintln!("{exact} representation(s) placed exactly the count they state");
}
