//! `PlaneSetRepresentation` — a set of planes, and the one that has no extent.
//!
//! # What the file states, and what it does not
//!
//! A plane set holds one or more `AbstractPlaneGeometry`, and RESQML 2.2 gives
//! that abstraction exactly two concrete forms. They are not equally drawable,
//! and the difference is the whole of this module:
//!
//! - **`TiltedPlaneGeometry`** carries `Plane`, one or more `ThreePoint3d`, and
//!   a `ThreePoint3d` is three `Point3d` — nine numbers that are *locations in
//!   the file*. The triangle through them is not an interpretation of the
//!   plane, it is three points the file states and the face they span. Nothing
//!   is invented, so it is drawn;
//! - **`HorizontalPlaneGeometry`** carries `Coordinate`: one number, a depth.
//!   A plane at that depth is infinite, and the file says nothing about how far
//!   it goes. That is [`Error::Unbounded`], not a gap in this build — no
//!   release will add an extent, because the extent is not a fact about the
//!   model. It is a decision about the picture, and it belongs to whoever
//!   draws. A viewer has the honest answer already: its own plane artefact,
//!   with an extent somebody chose.
//!
//! # Several triples under one `TiltedPlaneGeometry`
//!
//! The schema allows `Plane` to repeat (`min = 1`, unbounded) while the
//! documentation says *"the geometry of a tilted (or potentially not tilted)
//! plane from three points"* — singular. So a file with two triples under one
//! geometry can be read two ways: two planes, or one plane stated twice.
//!
//! **The ambiguity does not have to be resolved to draw honestly**, which is
//! why this does not refuse. Under either reading every triangle drawn is three
//! points the file states: if they are two planes, both are shown; if they are
//! one plane over-determined, both samples of it are shown. No vertex is
//! invented in either case. What would need the ambiguity settled is anything
//! that *computes* on the plane — an intersection, a normal for the set — and
//! nothing here does.
//!
//! # No CRS conversion here
//!
//! Like every other reader, the points come out in the representation's own
//! local system; the placement layer runs once over everything. Converting here
//! would flip a `ZIncreasingDownward` plane twice.

use energyml_core::value::Reflect;

use crate::error::{Error, Result};
use crate::geometry::Patch;
use crate::geometry::Topology;
use crate::points::Point;
use crate::source::ValueSource;
use crate::values::scalar_f64;

/// The field of `PlaneSetRepresentation` holding the planes.
///
/// A named constant beside the code that reads it, like `NODE_PATCH` and
/// `SPLIT_FIELDS`: a release that respells the concept costs one string here,
/// never a `match` on a version number.
const PLANES: &[&str] = &["Planes"];

/// The field of a `TiltedPlaneGeometry` holding its triples of points.
const TRIPLES: &[&str] = &["Plane"];

/// Read a plane set: one patch per plane the file gives points for.
///
/// `source` is unused and taken all the same: a plane set holds its numbers in
/// the XML, not in an external array, and a reader that dropped the argument
/// would have to be changed the day one of its forms grows a `Points`.
pub(crate) fn read(representation: &dyn Reflect, _source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let planes = first_list(representation, PLANES);
    if planes.is_empty() {
        return Err(Error::malformed(
            "PlaneSetRepresentation",
            "the representation declares no plane at all",
        ));
    }

    let mut out = Vec::new();
    let mut node_offset = 0usize;
    for (index, plane) in planes.iter().enumerate() {
        let at = format!("Planes[{index}]");
        let form = plane.type_info().type_key;
        let triples = first_list(*plane, TRIPLES);

        if triples.is_empty() {
            // The only other concrete form in 2.2 is the horizontal one, and it
            // is refused **by name and with what it does hold**: a depth. A
            // reader that skipped it would draw a set of three planes as two,
            // which is a picture that looks complete and is not.
            //
            // Read by the field present rather than by the class name, as
            // everywhere else: a release that adds a third form with points in
            // a `Plane` element is drawn by this reader without being listed,
            // and one without them is refused rather than silently dropped.
            return Err(Error::unbounded(
                crate::geometry::leak(form),
                at,
                "a depth, and nothing about how far the plane extends",
            ));
        }

        let mut points: Vec<Point> = Vec::new();
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        for (which, triple) in triples.iter().enumerate() {
            let corners = triple_points(*triple, &format!("{at}.Plane[{which}]"))?;
            let base = points.len() as u32;
            points.extend(corners);
            triangles.push([base, base + 1, base + 2]);
        }

        let start = node_offset;
        node_offset += points.len();
        out.push(Patch {
            index,
            node_offset: start,
            points,
            topology: Topology::Triangles(triangles),
            closed: Vec::new(),
        });
    }
    Ok(out)
}

/// The three points of a `ThreePoint3d`.
///
/// The schema pins the count at exactly three (`min = 3, max = 3`), so anything
/// else is a malformed file and is named as one. Reading the first three of a
/// longer list would draw a triangle the file did not state.
fn triple_points(triple: &dyn Reflect, at: &str) -> Result<[Point; 3]> {
    let listed = crate::geometry::field_list(triple, "Point3d");
    if listed.len() != 3 {
        return Err(Error::malformed(
            at,
            format!(
                "a `ThreePoint3d` states exactly three points, this one states {}",
                listed.len()
            ),
        ));
    }
    let mut corners = [[0.0f64; 3]; 3];
    for (index, point) in listed.iter().enumerate() {
        corners[index] = [
            coordinate(*point, "Coordinate1", at, index)?,
            coordinate(*point, "Coordinate2", at, index)?,
            coordinate(*point, "Coordinate3", at, index)?,
        ];
    }
    Ok(corners)
}

fn coordinate(point: &dyn Reflect, field: &str, at: &str, index: usize) -> Result<f64> {
    scalar_f64(point, field)
        .ok_or_else(|| Error::malformed(at, format!("point {index} states no `{field}`")))
}

/// The first of these fields that holds anything — the respelling rule.
fn first_list<'a>(object: &'a dyn Reflect, fields: &[&str]) -> Vec<&'a dyn Reflect> {
    for field in fields {
        let found = crate::geometry::field_list(object, field);
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}
