//! `GridConnectionSetRepresentation` - the connections a grid states between
//! its cells, and what can honestly be drawn for them.
//!
//! # What the standard says, and what it settles
//!
//! Read from the RESQML 2.2 Technical Reference Guide §5.25 and the
//! Technical Usage Guide §9.16, §9.5.3, §9.12 and §9.13. Three quotations decide
//! everything below.
//!
//! **The object has no geometry, by design.** TUG §9.16: *"Unlike a grid,
//! geometry as such is never attached to a connection, although geometric
//! properties, e.g., cell face overlap area, may be attached to a connection."*
//! So deriving the picture from the grids it points at is not a workaround; it
//! is the only road the standard leaves.
//!
//! **Its subject is the fault.** TRG §5.25: *"Grid connection sets are the
//! preferred means of representing faults on a grid. The use of cell-face-pairs
//! is more complete than single cell-faces, which are missing a corresponding
//! cell face entry, and only provide an incomplete representation of the
//! topology of a fault."*
//!
//! **But the face is optional in the file.** `LocalFacePerCellIndexPairs` is
//! `[0..1]`, and the TRG says when it is absent: *"If no face-per-cell
//! definition occurs as part of the grid representation, e.g., for a
//! block-centered grid, then this array need not appear."*
//!
//! # So the file decides, and each branch states exactly what it holds
//!
//! | what the file states | what is drawn |
//! |---|---|
//! | `LocalFacePerCellIndexPairs` present | **the named faces**, one per side |
//! | absent | a **segment between the two cell centres** |
//!
//! The second is not a fallback for the first. It is what the file says when it
//! says only that, and it is the *only* honest picture of a **non-neighbour
//! connection** - two cells that do not touch, between which no shared face
//! exists at all. The usage guide concedes as much: *"Even if a face does not
//! actually exist, most of the time it conceptually exists"* - conceptually,
//! which is not a place one can draw.
//!
//! # The local face ordering is specified for both kinds of grid
//!
//! This is what made the reader writable, and it was the part assumed missing:
//!
//! - **IJK** - TUG §9.12: *"there are 6 'faces per cell': 0=top, 1=bottom, 2-5
//!   are side faces following the IJ circular node ordering"*;
//! - **unstructured** - TUG §9.13: *"for a cell of the model with N faces as
//!   specified by the 'faces per cell' element, the faces are numbered from 0 to
//!   N-1 in this order"* - a direct index into `FacesPerCell`, which
//!   `unstructured.rs` already reads.
//!
//! # An alternate cell indexing changes what every number here means
//!
//! Found while checking this reader against the guide. RESQML 2.2 lets
//! an **unstructured** grid carry an `AlternateCellIndex`, and TRG §5.10 is
//! absolute about it: *"If defined, this alternate cell indexing is the only one
//! to rely on when referencing the representation cells."* TUG §9.10.2.1 names
//! this reader among the consequences - *"must be the only one considered for
//! any related grid cell objects, such as grid connection set"*.
//!
//! Nothing here resolves it, so such a grid is **refused by name**. Read against
//! the contiguous numbering, every pair would address a different cell and the
//! picture would be confident and wrong.
//!
//! # A cell is addressed by the number the *file* uses
//!
//! The trap this reader would have fallen into: `Topology::Hexahedra` compacts
//! its cells - an undefined one is not pushed - so a position is not the RESQML
//! cell index on a grid with holes. `Topology::position_of` is the translation,
//! and a cell it cannot place is a **named refusal** rather than a neighbour
//! drawn in its stead.

use energyml_core::value::Reflect;

use crate::error::{Error, Result};
use crate::geometry::{Geometry, Patch, Topology};
use crate::points::Point;
use crate::source::ValueSource;
use crate::values::read_integers;

/// The field by which a grid declares an **alternate cell indexing**.
///
/// RESQML 2.2 only, and on an unstructured grid only. `OriginalCellIndex` holds
/// an `AlternateCellIndex`, whose effect on this reader is total - TRG §5.10:
/// *"If defined, this alternate cell indexing is **the only one to rely on**
/// when referencing the representation cells"*, and the Usage Guide §9.10.2.1
/// names the consequence: *"this alternate cell indexing must be the only one
/// considered for any related grid cell objects, **such as grid connection
/// set**, subrepresentation, grid parentage, etc."*
///
/// Nothing in this workspace resolves it, so a connection set over such a grid
/// is **refused by name** rather than read against the contiguous numbering,
/// which would address a different cell for every pair and say nothing. The
/// guide calls its use *"strongly discouraged except if no other solution is
/// acceptable"*, and no package of the corpus has one - which is why refusing
/// is the right first answer and not a shortcut.
const ALTERNATE_INDEX: &[&str] = &["OriginalCellIndex"];

/// The field naming the grids a connection set draws from.
const GRIDS: &[&str] = &["Grid"];

/// The element saying which interpretation each connection belongs to.
///
/// One spelling in both releases. TRG §5.25 on why the array inside it is
/// jagged: *"The use of a RESQML jagged array allows zero or more than one
/// interpretation to be associated with a single connection"*, which the usage
/// guide motivates as *"multiple faults discretized onto a single connection"*.
///
/// And they are not necessarily faults. The guide is explicit that *"a
/// connection may also represent a horizon or geobody boundary"*, so nothing
/// here says "fault" - the feature says what it is, and this reader carries the
/// reference rather than a label it made up.
const INTERPRETATIONS: &[&str] = &["ConnectionInterpretations"];
/// The cell pairs: `2 × Count`, `(Cell1, Cell2)` per connection.
const CELL_PAIRS: &[&str] = &["CellIndexPairs"];
/// Which grid each cell of a pair belongs to. Absent when there is one grid.
const GRID_PAIRS: &[&str] = &["GridIndexPairs"];
/// The local face of each cell of a pair. Absent for a block-centred grid.
const FACE_PAIRS: &[&str] = &["LocalFacePerCellIndexPairs"];

/// Read a connection set: one patch, holding what every connection states.
///
/// The grids are read through the same `source`: `ArraySource` requires
/// `ObjectSource`, so a store that can hand out an array can hand out the grid
/// the array's indices are about. That is why this reader has the ordinary
/// signature despite resolving references - the same road `parent.rs` takes for
/// a local grid's parent.
pub(crate) fn read(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    Ok(build(representation, source)?.0)
}

/// The same patch, plus **which connection each drawn entry came from**.
///
/// The second half is the part that cannot be recovered afterwards, and the
/// reason this is one walk and not two. What is drawn is not one entry per
/// connection: with faces it is one ring per *side*, so two per connection, and
/// either branch drops a side whose cell the grid cannot place. Lining a
/// per-connection array up against the drawn entries by position would
/// therefore be right on a whole grid and silently shifted on the pierced ones -
/// the same failure this reader already had to disarm once, over cell
/// numbering.
fn build(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<(Vec<Patch>, Vec<u32>)> {
    let Some(pairs) = integers(representation, CELL_PAIRS, source)? else {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation",
            "no `CellIndexPairs`: the set states no connection",
        ));
    };
    if pairs.is_empty() {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation",
            "`CellIndexPairs` is empty: the set states no connection",
        ));
    }
    if pairs.len() % 2 != 0 {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation.CellIndexPairs",
            format!("{} indices cannot be read as pairs", pairs.len()),
        ));
    }
    let connections = pairs.len() / 2;

    // The grids, in the order the file lists them: `GridIndexPairs` indexes
    // *this* list, not the package.
    let grids = read_grids(representation, source)?;

    let faces = integers(representation, FACE_PAIRS, source)?;
    if let Some(found) = &faces
        && found.len() != pairs.len()
    {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation.LocalFacePerCellIndexPairs",
            format!(
                "{} face indices against {} cell indices: the schema pairs them one for one",
                found.len(),
                pairs.len()
            ),
        ));
    }

    let which = integers(representation, GRID_PAIRS, source)?;
    if let Some(found) = &which
        && found.len() != pairs.len()
    {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation.GridIndexPairs",
            format!(
                "{} grid indices against {} cell indices: the schema pairs them one for one",
                found.len(),
                pairs.len()
            ),
        ));
    }

    let mut points: Vec<Point> = Vec::new();
    let mut rings: Vec<Vec<u32>> = Vec::new();
    let mut segments: Vec<[u32; 2]> = Vec::new();
    let mut origin: Vec<u32> = Vec::new();
    let mut unplaced = 0usize;

    for connection in 0..connections {
        // **The two halves are gathered before anything is pushed.** Writing a
        // segment from "the last two points" is the mistake this workspace keeps
        // meeting: a side the grid cannot place pushes nothing, the count goes
        // odd, and the next connection's first centre is joined to the previous
        // connection's - every segment after a hole wearing its neighbour's end.
        let mut centres: [Option<Point>; 2] = [None, None];

        for (side, centre) in centres.iter_mut().enumerate() {
            let at = connection * 2 + side;
            // **A side that names no cell is a side that is not there.** Not a
            // failure and not a hole in the grid: the file says this connection
            // has one end, which is how a fault reaching the edge of a model is
            // written. It contributes nothing and is not counted as unplaced -
            // conflating the two would report a defect about a conformant file.
            let Some(cell) = pairs[at] else {
                continue;
            };
            let grid = which.as_ref().and_then(|list| list[at]).unwrap_or(0) as usize;
            let Some(shape) = grids.get(grid) else {
                return Err(Error::malformed(
                    "GridConnectionSetRepresentation.GridIndexPairs",
                    format!("grid index {grid} names no grid: the set lists {}", grids.len()),
                ));
            };

            // A named face of `None` on a side that *does* name a cell: the
            // file states the cell and not which of its faces. There is nothing
            // to draw and nothing to guess, so the side contributes nothing -
            // the same answer as a missing cell, for the same reason.
            let face = match faces.as_ref() {
                None => None,
                Some(list) => match list[at] {
                    Some(index) => Some(index as usize),
                    None => continue,
                },
            };

            match face {
                Some(local) => match face_of(shape, cell, local)? {
                    // A ring of any length, not a quadrilateral: an IJK cell's
                    // face has four corners, an unstructured cell's face has as
                    // many as the file gave it. Keeping only the four-cornered
                    // ones would drop a triangular fault face without a word.
                    //
                    // The ring is **not** wound for a positive signed area in
                    // XY, as `Topology::Polygons` asks of a map: a connection
                    // face is a fault face, so it is usually vertical and that
                    // area is zero. It keeps the winding the grid gave it, which
                    // is the one the cell states.
                    Some(face) => {
                        let base = points.len() as u32;
                        rings.push((0..face.len() as u32).map(|at| base + at).collect());
                        points.extend(face);
                        origin.push(connection as u32);
                    }
                    None => unplaced += 1,
                },
                None => match centre_of(shape, cell) {
                    Some(found) => *centre = Some(found),
                    None => unplaced += 1,
                },
            }
        }

        // Without faces, the connection **is** the segment between the two
        // centres - and only when both were placed. Half a segment is a line to
        // nowhere, and the file states a connection, not a point.
        if let [Some(a), Some(b)] = centres {
            let base = points.len() as u32;
            points.push(a);
            points.push(b);
            segments.push([base, base + 1]);
            origin.push(connection as u32);
        }
    }

    if points.is_empty() {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation",
            format!(
                "none of the {connections} connection(s) names a cell the grids place \
                 ({unplaced} side(s) unplaced)"
            ),
        ));
    }

    Ok((
        vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: if faces.is_some() {
                Topology::Polygons(rings)
            } else {
                Topology::Segments(segments)
            },
            closed: Vec::new(),
        }],
        origin,
    ))
}

/// One interpretation a connection set names, as the file names it.
///
/// A reference and nothing more. Resolving it is the caller's business - the
/// title is here so a list can be shown without a second lookup, and it is the
/// reference's own `Title`, not a name invented for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionFeature {
    /// The interpretation's uuid.
    pub uuid: String,
    /// Its `Title`, empty when the reference carries none.
    pub title: String,
}

/// Which interpretation every **drawn entry** belongs to.
///
/// The alignment is the point: `per_entry` has exactly one row per cell of the
/// patch `read` returns, in the same order, so it can be coloured like a
/// discrete per-cell property with no arithmetic in between. A row is a *list*
/// because the file's array is jagged - a connection may name none (an empty
/// row) or several (*"multiple faults discretized onto a single connection"*).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interpretations {
    /// The interpretations named, in the file's order - the order the indices
    /// index. Not the order they are drawn in, and not deduplicated.
    pub features: Vec<ConnectionFeature>,
    /// One row per drawn entry: the indices into [`features`].
    ///
    /// [`features`]: Interpretations::features
    pub per_entry: Vec<Vec<u32>>,
}

/// Read a connection set, and what each drawn entry is an occurrence of.
///
/// `Ok((patches, None))` when the set names no interpretation, which is the
/// common case and not a defect: a connection set is complete without one.
///
/// The two are returned together because they are one walk. Reading the
/// interpretations separately would mean placing every cell twice and, worse,
/// two placements that could disagree - a side dropped in one and kept in the
/// other shifts every row after it, and nothing would say so.
pub fn read_with_interpretations(
    representation: &dyn Reflect,
    source: &dyn ValueSource,
) -> Result<(Vec<Patch>, Option<Interpretations>)> {
    let (patches, origin) = build(representation, source)?;
    let Some(element) = INTERPRETATIONS
        .iter()
        .find_map(|field| crate::geometry::field_object(representation, field))
    else {
        return Ok((patches, None));
    };

    let features: Vec<ConnectionFeature> = crate::geometry::field_list(element, "FeatureInterpretation")
        .into_iter()
        .map(|reference| ConnectionFeature {
            uuid: energyml_core::identity::uuid_of_reflect(reference)
                .unwrap_or_default()
                .to_string(),
            title: text_of(reference, "Title").unwrap_or_default(),
        })
        .collect();
    if features.is_empty() {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation.ConnectionInterpretations",
            "interpretation indices with no `FeatureInterpretation` to index",
        ));
    }

    let Some(array) = crate::geometry::field_object(element, "InterpretationIndices") else {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation.ConnectionInterpretations",
            "no `InterpretationIndices`: nothing says which connection is which",
        ));
    };
    let rows = energyml_domain::values::jagged(
        array,
        source,
        "GridConnectionSetRepresentation.ConnectionInterpretations.InterpretationIndices",
    )?;

    // **The length check is most of the value of this function.** One row per
    // connection is what the schema says; a file that wrote one row per drawn
    // side, or per grid, produces a shorter or longer array that would still
    // index cleanly for a while and attribute the wrong fault from the first
    // discrepancy onward - a picture that is confident, colourful and wrong.
    let connections = origin.iter().copied().max().map_or(0, |last| last as usize + 1);
    if rows.len() < connections {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation.ConnectionInterpretations.InterpretationIndices",
            format!(
                "{} row(s) for {connections} connection(s): the schema gives one row per connection",
                rows.len()
            ),
        ));
    }

    let mut per_entry = Vec::with_capacity(origin.len());
    for connection in &origin {
        let Some(values) = rows[*connection as usize].integer() else {
            return Err(Error::malformed(
                "GridConnectionSetRepresentation.ConnectionInterpretations.InterpretationIndices",
                "the indices are not integers",
            ));
        };
        let mut row = Vec::with_capacity(values.len());
        for value in values {
            // Out of range names no interpretation, and guessing which one was
            // meant is exactly the invention this reader exists to avoid.
            if *value < 0 || *value as usize >= features.len() {
                return Err(Error::malformed(
                    "GridConnectionSetRepresentation.ConnectionInterpretations.InterpretationIndices",
                    format!(
                        "the index {value} names no interpretation: the set lists {}",
                        features.len()
                    ),
                ));
            }
            row.push(*value as u32);
        }
        per_entry.push(row);
    }

    Ok((patches, Some(Interpretations { features, per_entry })))
}

/// A string field of a reflected object, empty treated as absent.
fn text_of(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)?.as_scalar()? {
        energyml_core::value::Scalar::Str(found) if !found.is_empty() => Some(found.to_string()),
        _ => None,
    }
}

/// The grids the set draws from, read once each and in the file's order.
fn read_grids(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Geometry>> {
    let mut out = Vec::new();
    for field in GRIDS {
        for reference in crate::geometry::field_list(representation, field) {
            let uri = crate::follow::address_of(reference).ok_or_else(|| {
                Error::malformed(
                    "GridConnectionSetRepresentation.Grid",
                    "a grid reference with no uuid",
                )
            })?;
            // **A grid the package does not hold is a named refusal**, never a
            // connection quietly dropped: the set says these cells are joined,
            // and not being able to place them is news.
            let object = source
                .object_at(&uri)
                .ok_or_else(|| Error::Missing(uri.clone()))?;
            let grid = object.as_reflect();
            // Checked **before** the geometry is read, because the refusal is
            // about what the cell numbers mean and not about the shape: reading
            // the grid first would spend the work and then throw it away.
            if ALTERNATE_INDEX
                .iter()
                .any(|field| crate::geometry::field_object(grid, field).is_some())
            {
                return Err(Error::unsupported(
                    "AlternateCellIndex",
                    format!(
                        "the grid {uri} declares an alternate cell indexing, which is then the \
                         only one its connection set's indices may be read against"
                    ),
                ));
            }
            out.push(crate::geometry::of(grid, source)?);
        }
        if !out.is_empty() {
            break;
        }
    }
    if out.is_empty() {
        return Err(Error::malformed(
            "GridConnectionSetRepresentation",
            "the set names no grid, so its cell indices name nothing",
        ));
    }
    Ok(out)
}

/// The corners of one local face of one cell, in the file's own numbering.
///
/// `Ok(None)` for a cell the grid dropped - undefined, or with no located
/// pillar. That is an answer: the file names a cell the grid says has no shape.
/// An out-of-range **face** is a different matter and refuses by name, because
/// nothing about the file explains it.
fn face_of(shape: &Geometry, cell: u64, face: usize) -> Result<Option<Vec<Point>>> {
    for patch in &shape.patches {
        let Some(position) = patch.topology.position_of(cell as u32) else {
            continue;
        };
        return match &patch.topology {
            Topology::Hexahedra { cells, .. } => {
                let corners = cells[position];
                // TUG §9.12: 0 = top, 1 = bottom, 2-5 the sides in the IJ
                // circular node ordering. `Topology::Hexahedra` holds its eight
                // corners in VTK's order - face (0,1,2,3) against (4,5,6,7) -
                // which is the same pair of quadrilaterals, and the four sides
                // follow the same circular order.
                let quad: [usize; 4] = match face {
                    0 => [0, 1, 2, 3],
                    1 => [4, 7, 6, 5],
                    2 => [0, 4, 5, 1],
                    3 => [1, 5, 6, 2],
                    4 => [2, 6, 7, 3],
                    5 => [3, 7, 4, 0],
                    _ => {
                        return Err(Error::malformed(
                            "GridConnectionSetRepresentation.LocalFacePerCellIndexPairs",
                            format!("local face {face} on an IJK cell, which has six (0-5)"),
                        ));
                    }
                };
                Ok(Some(
                    quad.iter()
                        .map(|at| patch.points[corners[*at] as usize])
                        .collect(),
                ))
            }
            Topology::Polyhedra(cells) => {
                // TUG §9.13: the faces of a cell are numbered 0..N-1 in the
                // order `FacesPerCell` lists them, so the local index is a
                // direct offset into that cell's own run.
                let listed: Vec<usize> = cells.cell(position).map(|(face, _)| face).collect();
                let Some(global) = listed.get(face) else {
                    return Err(Error::malformed(
                        "GridConnectionSetRepresentation.LocalFacePerCellIndexPairs",
                        format!(
                            "local face {face} on a cell that has {} (0-{})",
                            listed.len(),
                            listed.len().saturating_sub(1)
                        ),
                    ));
                };
                Ok(Some(
                    cells
                        .face(*global)
                        .iter()
                        .map(|node| patch.points[*node as usize])
                        .collect(),
                ))
            }
            other => Err(Error::Shape {
                topology: crate::geometry::leak(name_of(other)),
                wanted: "a grid whose cells have faces",
            }),
        };
    }
    Ok(None)
}

/// The centre of a cell - the average of its corners.
///
/// `None` for a cell the grid dropped, for the same reason as above. The centre
/// is the honest anchor for a connection with no face: it is a point of the
/// cell, and the file states the cell.
fn centre_of(shape: &Geometry, cell: u64) -> Option<Point> {
    for patch in &shape.patches {
        let Some(position) = patch.topology.position_of(cell as u32) else {
            continue;
        };
        let nodes: Vec<u32> = match &patch.topology {
            Topology::Hexahedra { cells, .. } => cells[position].to_vec(),
            Topology::Polyhedra(cells) => cells
                .cell(position)
                .flat_map(|(face, _)| cells.face(face).to_vec())
                .collect(),
            _ => return None,
        };
        if nodes.is_empty() {
            return None;
        }
        let mut sum = [0.0f64; 3];
        for node in &nodes {
            let point = patch.points[*node as usize];
            for axis in 0..3 {
                sum[axis] += point[axis];
            }
        }
        let count = nodes.len() as f64;
        return Some([sum[0] / count, sum[1] / count, sum[2] / count]);
    }
    None
}

/// What a topology is called, for a refusal that names it.
fn name_of(topology: &Topology) -> &'static str {
    match topology {
        Topology::Points => "a set of points",
        Topology::Triangles(_) => "a set of triangles",
        Topology::Quads(_) => "a set of quadrilaterals",
        Topology::Polygons(_) => "a set of polygons",
        Topology::Polylines(_) => "a set of polylines",
        Topology::Segments(_) => "a set of segments",
        Topology::Grid { .. } => "a lattice",
        Topology::Hexahedra { .. } => "an IJK grid",
        Topology::Polyhedra(_) => "an unstructured grid",
    }
}

/// An integer array under the first of these fields that carries one.
///
/// `None` when none of them does, which the schema allows for two of the three
/// arrays here - absent is not an error, and telling that apart from an empty
/// array is the whole reason this returns an option.
///
/// A negative index is refused rather than cast: `CellIndexPairs` is declared
/// `PositiveLong`, and a negative one read as `u64` becomes an enormous cell
/// number that no grid places - a connection silently dropped instead of a file
/// reported.
fn integers(
    object: &dyn Reflect,
    fields: &[&str],
    source: &dyn ValueSource,
) -> Result<Option<Vec<Option<u64>>>> {
    for field in fields {
        let Some(array) = crate::geometry::field_object(object, field) else {
            continue;
        };
        // **The array's own null value, and it is not decoration here.** A
        // connection may name a cell on one side and nothing on the other -
        // TRG §5.25 calls these *"single cell-faces, which are missing a
        // corresponding cell face entry"*, and they are how a fault reaching
        // the edge of a model is written. `testingPackageCpp` has three
        // connections and two of them are that shape: cells `[0, 9999, 0, 1,
        // 9999, 1]` against `NullValue 9999`, faces `[3, -1, 3, 5, -1, 5]`
        // against `NullValue -1`.
        //
        // Read without it, the `-1` refused the whole set by name and the
        // `9999` would have been looked up as a cell - the second being the
        // worse of the two, because a grid with 10 000 cells would have found
        // one and drawn a face nobody named.
        let null = crate::values::scalar_i64(array, "NullValue");
        let read = read_integers(array, source, field)?;
        let mut out = Vec::with_capacity(read.len());
        for value in read {
            if Some(value) == null {
                out.push(None);
                continue;
            }
            let Ok(index) = u64::try_from(value) else {
                return Err(Error::malformed(
                    format!("GridConnectionSetRepresentation.{field}"),
                    format!("{value} is not an index: the schema makes these positive"),
                ));
            };
            out.push(Some(index));
        }
        return Ok(Some(out));
    }
    Ok(None)
}
