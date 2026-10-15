//! `GridConnectionSetRepresentation` - what the file states, and nothing else.
//!
//! No package of the corpus holds one, so the grid and the connection set are
//! built here. The workshop rule applies in its strict form: **the shape of the
//! assertions comes from the spec**, and every decision quoted is the TRG 2.2
//! §5.25's or the Technical Usage Guide's §9.12, §9.13, §9.16, §5.25.
//!
//! Four traps are guarded, and each one is a way of looking right:
//!
//! - a cell addressed by its **position** instead of its number, which is right
//!   on a whole grid and wrong on a pierced one - that is, on exactly the grids
//!   a connection set exists for;
//! - a segment drawn from "the last two points", which joins two different
//!   connections as soon as a side could not be placed;
//! - a three-sided face silently dropped for not having four corners;
//! - an interpretation lined up against the drawn entries **by position**, which
//!   is the same trap one layer up: what is drawn is one entry per *side*, and a
//!   dropped side shifts every fault name after it.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_geometry::{Topology, geometry};
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};

use crate::build;

use build::{CELLS, DEFINED, FACES, GRID, INTERP, LENGTHS, POINTS, PROXY, WHICH};

/// 3 × 2 pillars over 2 levels: two cells along I, every coordinate distinct so
/// that a cell reached for sideways gives another number rather than the same
/// one by luck.
fn pillars() -> Vec<f64> {
    let mut out = Vec::new();
    for k in 0..2 {
        for j in 0..2 {
            for i in 0..3 {
                out.extend([100.0 * i as f64, 50.0 * j as f64, 200.0 * k as f64]);
            }
        }
    }
    out
}

/// A store holding the grid and whatever arrays the set points at.
struct Store {
    /// `CellGeometryIsDefined`, one flag per cell, or nothing at all.
    defined: Option<Vec<i64>>,
    doubles: std::collections::HashMap<String, (Vec<f64>, Vec<i64>)>,
    integers: std::collections::HashMap<String, (Vec<i64>, Vec<i64>)>,
}

impl Store {
    fn new(pairs: &[(i64, i64)], faces: Option<&[i64]>, which: Option<&[i64]>) -> Self {
        let mut doubles = std::collections::HashMap::new();
        doubles.insert(POINTS.to_string(), (pillars(), vec![2, 2, 3, 3]));

        let mut integers = std::collections::HashMap::new();
        let flat: Vec<i64> = pairs.iter().flat_map(|(a, b)| [*a, *b]).collect();
        let count = flat.len() as i64;
        integers.insert(CELLS.to_string(), (flat, vec![count / 2, 2]));
        if let Some(list) = faces {
            integers.insert(FACES.to_string(), (list.to_vec(), vec![list.len() as i64]));
        }
        if let Some(list) = which {
            integers.insert(WHICH.to_string(), (list.to_vec(), vec![list.len() as i64]));
        }

        Store {
            defined: None,
            doubles,
            integers,
        }
    }

    /// Give the store the jagged array a `ConnectionInterpretations` points at.
    ///
    /// `cumulative` is the *inclusive* running total with no leading zero, which
    /// is the form RESQML stores and the form a real file would hold.
    fn with_interpretations(mut self, flat: &[i64], cumulative: &[i64]) -> Self {
        self.integers
            .insert(INTERP.to_string(), (flat.to_vec(), vec![flat.len() as i64]));
        self.integers.insert(
            LENGTHS.to_string(),
            (cumulative.to_vec(), vec![cumulative.len() as i64]),
        );
        self
    }

    /// Mark cell 0 undefined - the hole that makes a position stop being a number.
    fn with_hole(mut self) -> Self {
        self.defined = Some(vec![0, 1]);
        self
    }
}

impl ObjectSource for Store {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        (uuid == GRID).then(|| build::boxed(build::grid(self.defined.is_some())))
    }
}

impl ArraySource for Store {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        build::dataset_of(object)
            .map(|path| ArrayLocation {
                file: "memory".to_string(),
                path_in_file: path,
                mime: None,
                start: Vec::new(),
                count: Vec::new(),
                proxy_uuid: Some(PROXY.to_string()),
                field_path: "Values".to_string(),
                resource_uri: None,
            })
            .into_iter()
            .collect()
    }

    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        if let Some((_, shape)) = self.doubles.get(&location.path_in_file) {
            return Ok(ArrayMetadata::new(shape.clone(), "double".to_string()));
        }
        if location.path_in_file == DEFINED {
            return Ok(ArrayMetadata::new(vec![2], "int64".to_string()));
        }
        let (_, shape) = self
            .integers
            .get(&location.path_in_file)
            .ok_or_else(|| energyml_domain::Error::Missing(location.path_in_file.clone()))?;
        Ok(ArrayMetadata::new(shape.clone(), "int64".to_string()))
    }

    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        let metadata = self.metadata(location)?;
        if let Some((values, _)) = self.doubles.get(&location.path_in_file) {
            return Ok(ArrayData {
                metadata,
                values: ArrayValues::F64(values.clone()),
            });
        }
        if location.path_in_file == DEFINED {
            // Absent, every cell is defined: the flag is optional, and a grid
            // that states none states nothing against its cells.
            let flags = self.defined.clone().unwrap_or_else(|| vec![1, 1]);
            return Ok(ArrayData {
                metadata,
                values: ArrayValues::I64(flags),
            });
        }
        let (values, _) = self
            .integers
            .get(&location.path_in_file)
            .ok_or_else(|| energyml_domain::Error::Missing(location.path_in_file.clone()))?;
        Ok(ArrayData {
            metadata,
            values: ArrayValues::I64(values.clone()),
        })
    }

    fn read_into(
        &self,
        location: &ArrayLocation,
        _into: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        self.metadata(location)
    }
}

#[test]
fn without_faces_a_connection_is_the_segment_between_two_cell_centres() {
    // TUG §9.16: *"geometry as such is never attached to a connection"*. With
    // no face named, all the file states is "these two cells are joined" - the
    // segment between their centres says that and invents nothing.
    let store = Store::new(&[(0, 1)], None, None);
    let set = build::connections(&[(0, 1)], None, None);
    let found = geometry::of(set.as_reflect(), &store).expect("the set reads");

    let patch = &found.patches[0];
    assert_eq!(patch.points.len(), 2, "one centre per side");
    assert_eq!(patch.topology, Topology::Segments(vec![[0, 1]]));
    // The two cell centres: x = 50 and x = 150, everything else alike.
    assert!((patch.points[0][0] - 50.0).abs() < 1e-9, "{:?}", patch.points[0]);
    assert!((patch.points[1][0] - 150.0).abs() < 1e-9, "{:?}", patch.points[1]);
}

#[test]
fn with_faces_the_named_face_of_each_cell_is_drawn() {
    // TUG §9.12: *"0=top, 1=bottom, 2-5 are side faces following the IJ
    // circular node ordering"*. A named face is four corners of the cell, so
    // four points per side and one ring each.
    let store = Store::new(&[(0, 1)], Some(&[3, 5]), None);
    let set = build::connections(&[(0, 1)], Some(&[3, 5]), None);
    let found = geometry::of(set.as_reflect(), &store).expect("the set reads");

    let patch = &found.patches[0];
    assert_eq!(patch.points.len(), 8, "four corners per side");
    match &patch.topology {
        Topology::Polygons(rings) => {
            assert_eq!(rings.len(), 2, "one ring per side of the connection");
            assert!(rings.iter().all(|ring| ring.len() == 4));
            // **The rings do not overlap**: the second is indexed on its own
            // points. Indexed like the first, it would draw the same face
            // twice, with the right ring count and the right point count - the
            // shape of mistake no count catches.
            assert_eq!(rings[0], vec![0, 1, 2, 3]);
            assert_eq!(rings[1], vec![4, 5, 6, 7]);
        }
        other => panic!("rings were expected, not {other:?}"),
    }
}

#[test]
fn a_cell_the_grid_dropped_is_not_a_neighbouring_cell() {
    // **The central trap.** `Topology::Hexahedra` compacts its cells, so a
    // position is not the RESQML number as soon as a grid has a hole. Here cell
    // 0 is marked undefined: connection (0,1) therefore has only one placeable
    // side, and nothing in the file lets a segment be drawn. A reader indexing
    // on the position would have taken cell 1 for cell 0 and drawn a segment
    // from cell 1 to itself.
    let store = Store::new(&[(0, 1)], None, None).with_hole();
    let set = build::connections(&[(0, 1)], None, None);

    let error = geometry::of(set.as_reflect(), &store).unwrap_err().to_string();
    assert!(error.contains("names a cell the grids place"), "{error}");
    assert!(error.contains("1 side(s) unplaced"), "{error}");
}

#[test]
fn a_side_that_cannot_be_placed_never_joins_the_next_connection() {
    // The second trap: a segment drawn from "the last two points" joins two
    // different connections as soon as a side is missing. Here cell 0 is
    // undefined and two connections name it on one side - so **no** segment
    // may come out, and above all not one joining the two surviving halves,
    // which belong to different connections.
    let store = Store::new(&[(0, 1), (1, 0)], None, None).with_hole();
    let set = build::connections(&[(0, 1), (1, 0)], None, None);

    let error = geometry::of(set.as_reflect(), &store).unwrap_err().to_string();
    assert!(error.contains("2 side(s) unplaced"), "{error}");
}

#[test]
fn a_local_face_outside_the_cell_refuses_by_name() {
    // An IJK cell has six faces (TUG §9.12). An index of 6 is explained by
    // nothing in the file, so it is refused rather than folded back into range
    // - a modulo would have drawn the top face for face 6.
    let store = Store::new(&[(0, 1)], Some(&[6, 0]), None);
    let set = build::connections(&[(0, 1)], Some(&[6, 0]), None);

    let error = geometry::of(set.as_reflect(), &store).unwrap_err().to_string();
    assert!(error.contains("local face 6"), "{error}");
    assert!(error.contains("six"), "{error}");
}

#[test]
fn the_two_arrays_must_pair_one_for_one() {
    // The schema pairs them: `2 × #Connections` on either side. One face per
    // connection instead of two would read without error and lay the second
    // side's face onto the first.
    let store = Store::new(&[(0, 1)], Some(&[3]), None);
    let set = build::connections(&[(0, 1)], Some(&[3]), None);

    let error = geometry::of(set.as_reflect(), &store).unwrap_err().to_string();
    assert!(error.contains("pairs them one for one"), "{error}");
}

#[test]
fn a_grid_the_package_does_not_hold_is_a_named_refusal() {
    // The set says these cells are joined; not being able to place them is
    // news, not an empty object.
    struct Empty(Store);
    impl ObjectSource for Empty {
        fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
            None
        }
    }
    impl ArraySource for Empty {
        fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
            self.0.locate(object)
        }
        fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
            self.0.metadata(location)
        }
        fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
            self.0.read(location)
        }
        fn read_into(
            &self,
            location: &ArrayLocation,
            into: &mut ArrayBuffer<'_>,
        ) -> energyml_domain::Result<ArrayMetadata> {
            self.0.read_into(location, into)
        }
    }

    let set = build::connections(&[(0, 1)], None, None);
    let error = geometry::of(set.as_reflect(), &Empty(Store::new(&[(0, 1)], None, None)))
        .unwrap_err()
        .to_string();
    assert!(error.contains(GRID), "{error}");
}

#[test]
fn the_reader_is_registered_under_the_concept() {
    let set = build::connections(&[(0, 1)], None, None);
    assert!(geometry::is_supported(set.as_reflect()));
}

#[test]
fn every_drawn_ring_carries_the_interpretation_of_the_connection_it_came_from() {
    // TRG §5.25: the jagged array exists so that a connection may name zero,
    // one or several interpretations - *"multiple faults discretized onto a
    // single connection"*. Here connection 0 names one, connection 1 names two,
    // and each side of a connection is drawn as its own ring, so the answer has
    // one row per **ring** and not one per connection.
    let store =
        Store::new(&[(0, 1), (1, 0)], Some(&[3, 5, 5, 3]), None).with_interpretations(&[0, 0, 1], &[1, 3]);
    let set = build::interpreted(
        build::connections(&[(0, 1), (1, 0)], Some(&[3, 5, 5, 3]), None),
        &[GRID, PROXY],
    );

    let (patches, found) = energyml_geometry::connection::read_with_interpretations(set.as_reflect(), &store)
        .expect("the set reads");
    let found = found.expect("the set names its interpretations");

    assert_eq!(found.features.len(), 2);
    assert_eq!(found.features[0].uuid, GRID);
    assert_eq!(
        found.features[0].title,
        format!("fault {GRID}"),
        "the reference's own title, not an invented one"
    );

    assert_eq!(
        patches[0].topology.cell_count(),
        4,
        "two sides drawn for each of two connections"
    );
    assert_eq!(
        found.per_entry.len(),
        patches[0].topology.cell_count(),
        "one row per drawn entry, so it lines up with the cells without arithmetic"
    );
    assert_eq!(found.per_entry, vec![vec![0], vec![0], vec![0, 1], vec![0, 1]]);
}

#[test]
fn a_dropped_side_shifts_nothing() {
    // **The test this feature exists for.** With a hole in the grid, one side of
    // one connection is not drawn - so the drawn entries and the file's
    // connections stop being the same list. A reader that walked the
    // interpretations separately, or lined them up by position, would give every
    // ring after the hole its neighbour's fault: the right number of rows, a
    // plausible picture, and wrong.
    //
    // Cell 0 is undefined and connection 0 names it on both sides, so connection
    // 0 draws nothing at all and only connection 1's single placeable side
    // survives. The one row that comes out must be connection 1's.
    let store = Store::new(&[(0, 0), (1, 0)], Some(&[3, 5, 5, 3]), None)
        .with_hole()
        .with_interpretations(&[0, 1], &[1, 2]);
    let set = build::interpreted(
        build::connections(&[(0, 0), (1, 0)], Some(&[3, 5, 5, 3]), None),
        &[GRID, PROXY],
    );

    let (patches, found) = energyml_geometry::connection::read_with_interpretations(set.as_reflect(), &store)
        .expect("the set reads");
    let found = found.expect("the set names its interpretations");

    assert_eq!(
        patches[0].topology.cell_count(),
        1,
        "only one side of one connection is placeable"
    );
    assert_eq!(
        found.per_entry,
        vec![vec![1]],
        "the surviving ring belongs to connection 1, whose interpretation is the second one"
    );
}

#[test]
fn an_index_that_names_no_interpretation_refuses_by_name() {
    // Nothing in the file explains it, and picking the nearest one would attach
    // a fault name to a connection that never claimed it.
    let store = Store::new(&[(0, 1)], None, None).with_interpretations(&[7], &[1]);
    let set = build::interpreted(build::connections(&[(0, 1)], None, None), &[GRID]);

    let error = energyml_geometry::connection::read_with_interpretations(set.as_reflect(), &store)
        .unwrap_err()
        .to_string();
    assert!(error.contains("names no interpretation"), "{error}");
    assert!(error.contains("the set lists 1"), "{error}");
}

#[test]
fn fewer_rows_than_connections_refuses_rather_than_guessing_the_rest() {
    // The length mismatch that would otherwise index cleanly for a while: two
    // connections, one row. Reading it would attribute the first fault to the
    // first connection and then run off the end - or, worse in another shape,
    // silently repeat.
    let store = Store::new(&[(0, 1), (1, 0)], None, None).with_interpretations(&[0], &[1]);
    let set = build::interpreted(build::connections(&[(0, 1), (1, 0)], None, None), &[GRID]);

    let error = energyml_geometry::connection::read_with_interpretations(set.as_reflect(), &store)
        .unwrap_err()
        .to_string();
    assert!(error.contains("one row per connection"), "{error}");
}

#[test]
fn a_set_without_interpretations_is_read_and_says_so() {
    // Not a defect and not a refusal: a connection set is complete without one.
    let store = Store::new(&[(0, 1)], None, None);
    let set = build::connections(&[(0, 1)], None, None);

    let (patches, found) = energyml_geometry::connection::read_with_interpretations(set.as_reflect(), &store)
        .expect("the set reads");
    assert_eq!(patches[0].topology.cell_count(), 1);
    assert!(found.is_none());
}
