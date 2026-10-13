//! Unstructured grids: cells made of faces, faces made of nodes.
//!
//! # The model, and how it differs from IJK
//!
//! An IJK grid says where a cell is by *arithmetic* - cell `(i,j,k)` takes its
//! corners from four pillars at two levels, and the reader computes the
//! indices. An unstructured grid says it by *enumeration*, in two hops:
//!
//! ```text
//! cell  --FacesPerCell-->  face  --NodesPerFace-->  node  --Points-->  xyz
//! ```
//!
//! Both hops are jagged arrays, because neither the faces per cell nor the
//! nodes per face are constant: the corpus grid is one **tetrahedron** and one
//! **prism**, four faces and five, triangles and quadrilaterals, in the same
//! two-cell grid.
//!
//! # Orientation belongs to the pair, not to the face
//!
//! A face between two cells is stored **once**, and its node order winds one
//! way. That order turns the right-hand normal out of one of the two cells and
//! into the other - so "which way does this face point" has no answer until you
//! say *for which cell*.
//!
//! RESQML handles that by making `CellFaceIsRightHanded` parallel to
//! `FacesPerCell`'s **elements**, not to the faces: *"Array length is the sum
//! of the cell face count per cell, and the data follows the order of the faces
//! per cell … list-of-lists."* So it holds one flag per cell-face **slot**, and
//! the corpus proves the point - face 0 is shared by both cells and carries
//! `true` in cell 0's slot and `false` in cell 1's.
//!
//! Reading it as one flag per face instead of per slot costs nothing on a grid
//! with no shared faces and turns every interior face inside out on a grid that
//! has them. It is a length check away, and the length check is done.
//!
//! # `CellShape` is a hint, and the corpus shows why
//!
//! *"This information is supplied by the RESQML writer to indicate the
//! complexity of the grid geometry, as an aide to the RESQML reader … BUSINESS
//! RULE: Should be consistent with the actual geometry of the grid."*
//!
//! **Should**, not must - and fesapi's own example grid declares `prism` while
//! holding a tetrahedron and a prism. A reader that trusted it and allocated
//! six nodes per cell would be wrong on half this file, with no schema
//! violation to point at. So the shape is never read here; the counts come from
//! the cumulative lengths, which are the data.
//!
//! # What the two versions cost: nothing
//!
//! Unusually, nothing at all. 2.2 renamed `HingeNodeFaces` to
//! `UnstructuredGridHingeNodeFaces` and `SubnodeTopology` to
//! `UnstructuredSubnodeTopology`, and reordered the geometry's children - but
//! every element this reader touches (`Points`, `NodeCount`, `FaceCount`,
//! `NodesPerFace`, `FacesPerCell`, `CellFaceIsRightHanded`) kept its name, and
//! reflection does not care about order. The jagged array changed type,
//! `resqml:ResqmlJaggedArray` to `eml:JaggedArray`, and kept both its field
//! names - so even that is invisible here.
//!
//! # What is not read
//!
//! **Hinge node faces** and **subnodes**, both optional and both about *higher
//! order* geometry: they refine the shape of a face beyond its corners. The
//! base geometry is complete without them, which the schema says outright -
//! *"only expected to be used if the hinge node faces higher order grid point
//! attachment arises"*. Nothing in the corpus carries either.

use energyml_core::value::Reflect;
use energyml_domain::values;

use crate::error::{Error, Result};
use crate::geometry::{Patch, Polyhedra, Topology, field_object};
use crate::source::ValueSource;
use crate::values::scalar_i64;

/// Read an `UnstructuredGridRepresentation` into one patch of polyhedra.
pub fn read(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let at = "UnstructuredGridRepresentation";
    let cells = match scalar_i64(representation, "CellCount") {
        Some(count) if count > 0 => count as usize,
        Some(count) => return Err(Error::malformed(at, format!("`CellCount` is {count}"))),
        None => return Err(Error::malformed(at, "no `CellCount`")),
    };

    let Some(geometry) = field_object(representation, "Geometry") else {
        // Legal, and the schema advertises it: "characterized by a cell count,
        // and potentially nothing else". Such a grid is an index space for
        // properties to hang on, with no shape at all - so it is named rather
        // than returned as an empty drawing.
        return Err(Error::unsupported(
            "an unstructured grid with a cell count and no geometry",
            format!("{at}, {cells} cell(s)"),
        ));
    };

    let Some(array) = field_object(geometry, "Points") else {
        return Err(Error::malformed(at, "a geometry with no `Points`"));
    };
    let points = crate::points::read(
        array,
        source,
        &format!("{at}.Geometry.Points"),
        crate::parametric::Vertical::read(geometry, source),
    )?;

    // Declared counts, checked against what the arrays actually hold. Both are
    // required and positive by the schema, so a mismatch is the file being
    // wrong rather than this reader guessing.
    let nodes = declared(geometry, "NodeCount", at)?;
    let faces = declared(geometry, "FaceCount", at)?;
    if points.len() != nodes {
        return Err(Error::malformed(
            at,
            format!("`NodeCount` is {nodes} and `Points` holds {}", points.len()),
        ));
    }

    let (face_nodes, face_starts) = jagged(geometry, "NodesPerFace", source, at)?;
    if face_starts.len().saturating_sub(1) != faces {
        return Err(Error::malformed(
            at,
            format!(
                "`FaceCount` is {faces} and `NodesPerFace` describes {}",
                face_starts.len().saturating_sub(1)
            ),
        ));
    }
    if let Some(bad) = face_nodes.iter().find(|node| **node as usize >= nodes) {
        return Err(Error::malformed(
            at,
            format!("`NodesPerFace` names node {bad} of {nodes}"),
        ));
    }

    let (cell_faces, cell_starts) = jagged(geometry, "FacesPerCell", source, at)?;
    if cell_starts.len().saturating_sub(1) != cells {
        return Err(Error::malformed(
            at,
            format!(
                "`CellCount` is {cells} and `FacesPerCell` describes {}",
                cell_starts.len().saturating_sub(1)
            ),
        ));
    }
    if let Some(bad) = cell_faces.iter().find(|face| **face as usize >= faces) {
        return Err(Error::malformed(
            at,
            format!("`FacesPerCell` names face {bad} of {faces}"),
        ));
    }

    let outward = handedness(geometry, source, cell_faces.len(), at)?;

    let polyhedra = Polyhedra::new(face_nodes, face_starts, cell_faces, cell_starts, outward);
    // A cell with fewer than four faces encloses nothing. Reported as a whole
    // rather than one message per cell, because a file that gets this wrong
    // gets it wrong everywhere.
    let degenerate = (0..polyhedra.cell_count())
        .filter(|cell| polyhedra.cell(*cell).count() < 4)
        .count();
    if degenerate == polyhedra.cell_count() && degenerate > 0 {
        return Err(Error::malformed(
            at,
            format!("all {degenerate} cell(s) have fewer than four faces, so enclose nothing"),
        ));
    }

    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points,
        topology: Topology::Polyhedra(polyhedra),
        closed: Vec::new(),
    }])
}

/// A required positive count.
fn declared(geometry: &dyn Reflect, field: &str, at: &str) -> Result<usize> {
    match scalar_i64(geometry, field) {
        Some(value) if value > 0 => Ok(value as usize),
        Some(value) => Err(Error::malformed(at, format!("`{field}` is {value}"))),
        None => Err(Error::malformed(at, format!("a geometry with no `{field}`"))),
    }
}

/// A RESQML jagged array, as a flat run plus starts.
///
/// The stored `CumulativeLength` is an *inclusive* running total with no
/// leading zero - 2.2's own worked example says so: elements `(a b c d e f g
/// h)` with lengths `(3 7 8)`. What every consumer wants is the exclusive form
/// with both ends, so the conversion happens here, once, rather than at each
/// index.
fn jagged(
    geometry: &dyn Reflect,
    field: &str,
    source: &dyn ValueSource,
    at: &str,
) -> Result<(Vec<u32>, Vec<u32>)> {
    let Some(array) = field_object(geometry, field) else {
        return Err(Error::malformed(at, format!("a geometry with no `{field}`")));
    };
    let rows = values::jagged(array, source, &format!("{at}.{field}"))?;
    let mut flat = Vec::new();
    let mut starts = Vec::with_capacity(rows.len() + 1);
    starts.push(0u32);
    for row in &rows {
        let Some(values) = row.integer() else {
            return Err(Error::malformed(
                at,
                format!("`{field}` holds something that is not indices"),
            ));
        };
        for value in values {
            if *value < 0 {
                return Err(Error::malformed(at, format!("`{field}` holds the index {value}")));
            }
            flat.push(*value as u32);
        }
        starts.push(flat.len() as u32);
    }
    Ok((flat, starts))
}

/// `CellFaceIsRightHanded`, one flag per cell-face slot.
///
/// The length check is the whole value of this function. A file that wrote one
/// flag per *face* rather than per slot produces an array that is shorter by
/// the number of shared faces, and reading it anyway would turn every interior
/// face of the grid inside out - visible as a cell that renders as a hole, and
/// invisible in any count.
fn handedness(geometry: &dyn Reflect, source: &dyn ValueSource, slots: usize, at: &str) -> Result<Vec<bool>> {
    let Some(array) = field_object(geometry, "CellFaceIsRightHanded") else {
        return Err(Error::malformed(at, "a geometry with no `CellFaceIsRightHanded`"));
    };
    let flags = values::read(array, source, &format!("{at}.CellFaceIsRightHanded"))?;
    let flags = flags
        .boolean()
        .ok_or_else(|| Error::malformed(at, "`CellFaceIsRightHanded` is not a boolean array"))?
        .to_vec();
    if flags.len() != slots {
        return Err(Error::malformed(
            at,
            format!(
                "`CellFaceIsRightHanded` holds {} flag(s) for {slots} cell-face pair(s): the \
                 schema says its length is \"the sum of the cell face count per cell\"",
                flags.len()
            ),
        ));
    }
    Ok(flags)
}
