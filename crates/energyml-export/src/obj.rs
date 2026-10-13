//! Wavefront OBJ - triangles and polylines, in metres.
//!
//! The simplest useful export: every mesh viewer reads it, it is text, and it
//! carries exactly what geometry has. What it does not carry is a coordinate
//! system, so the numbers are placed and normalised to metres before writing -
//! an OBJ of eastings in feet is a file nobody can combine with anything.
//!
//! The identity goes into comments. OBJ has no metadata section, and a `#` line
//! is the only place the uuid can live where every reader will ignore it and
//! every person will see it.

use std::io::Write;

use energyml_geometry::{Placement, SceneObject, Topology};

use crate::error::{Error, Result};
use crate::registry::{Exporter, Options};

/// Writes Wavefront OBJ.
pub struct ObjExporter;

impl Exporter for ObjExporter {
    fn name(&self) -> &'static str {
        "obj"
    }

    fn extension(&self) -> &'static str {
        "obj"
    }

    fn mime(&self) -> &'static str {
        "model/obj"
    }

    fn describe(&self) -> &'static str {
        "Wavefront OBJ: triangles and polylines, placed and in metres"
    }

    fn placement(&self) -> Placement {
        Placement::Metres
    }

    fn handles(&self, object: &SceneObject) -> bool {
        !object.geometry.is_empty()
    }

    fn write(&self, object: &SceneObject, out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
        if object.geometry.is_empty() {
            return Err(Error::unsuitable(
                "obj",
                format!("`{}`, which has no points", object.title),
            ));
        }
        self.write_all(std::slice::from_ref(object), out, options)
    }

    fn write_all(&self, objects: &[SceneObject], out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
        // One implementation, and `write` goes through it too. OBJ vertex
        // indices are **1-based and file-global**, so writing several objects
        // means carrying a running offset that a per-object function cannot
        // know about - and two code paths for one format is how the single-object
        // header came to say less than the multi-object one.
        let mut base = 1u32;
        for object in objects {
            if !self.handles(object) {
                continue;
            }
            // Reshaped as the caller's rules ask, then stripped of the nodes
            // that have no position - one funnel, one order, for every format.
            // `v NaN NaN NaN` is not something an OBJ reader can place, and a
            // face indexed on one has no shape. See `registry::prepare`.
            let object = crate::registry::prepare(object, options)?;
            let object = object.as_ref();
            let placement = options.placement.unwrap_or_else(|| self.placement());
            let points = options.points(object, placement)?;
            header(object, placement, options, out)?;
            writeln!(out, "o {}", sanitise(&object.title, &object.uuid))?;
            for point in &points {
                writeln!(out, "v {} {} {}", point[0], point[1], point[2])?;
            }
            for patch in &object.geometry.patches {
                match &patch.topology {
                    Topology::Triangles(triangles) => {
                        for triangle in triangles {
                            writeln!(
                                out,
                                "f {} {} {}",
                                triangle[0] + base,
                                triangle[1] + base,
                                triangle[2] + base
                            )?;
                        }
                    }
                    // A wireframe's edge: OBJ's `l` takes any two indices,
                    // consecutive or not.
                    Topology::Segments(pairs) => {
                        for pair in pairs {
                            writeln!(out, "l {} {}", pair[0] + base, pair[1] + base)?;
                        }
                    }
                    Topology::Polylines(runs) => {
                        let mut start = base;
                        for (rank, run) in runs.iter().enumerate() {
                            if *run >= 2 {
                                let mut indices: Vec<String> =
                                    (0..*run).map(|i| (start + i).to_string()).collect();
                                // A ring the file calls closed ends on its first
                                // vertex: an OBJ `l` has no closing flag.
                                if patch.closed.get(rank) == Some(&true) {
                                    indices.push(start.to_string());
                                }
                                writeln!(out, "l {}", indices.join(" "))?;
                            }
                            start += run;
                        }
                    }
                    // A cell becomes its six faces. Interior faces are written
                    // twice - once for each cell that owns them - which is
                    // wasteful and correct; deduplicating them would mean
                    // deciding which side is outside, and a faulted grid has no
                    // single answer to that.
                    Topology::Hexahedra { cells, .. } => {
                        for cell in cells {
                            for face in HEXAHEDRON_FACES {
                                writeln!(
                                    out,
                                    "f {} {} {} {}",
                                    cell[face[0]] + base,
                                    cell[face[1]] + base,
                                    cell[face[2]] + base,
                                    cell[face[3]] + base
                                )?;
                            }
                        }
                    }
                    // An unstructured cell becomes its own faces, whatever
                    // shape they are - OBJ takes a polygon of any size, so a
                    // quadrilateral stays one instead of becoming two
                    // triangles. Already wound outward, so the normals a viewer
                    // computes point out of the model rather than half in and
                    // half out.
                    Topology::Polyhedra(cells) => {
                        for cell in 0..cells.cell_count() {
                            for face in cells.faces_outward(cell) {
                                let nodes: Vec<String> =
                                    face.iter().map(|node| (node + base).to_string()).collect();
                                writeln!(out, "f {}", nodes.join(" "))?;
                            }
                        }
                    }
                    // OBJ takes a polygon of any size, so a quadrilateral goes
                    // in whole rather than becoming two triangles. Already
                    // wound by `energyml_geometry::shape`, which measures the
                    // signed area instead of assuming a convention the file
                    // does not state.
                    Topology::Quads(quads) => {
                        for quad in quads {
                            writeln!(
                                out,
                                "f {} {} {} {}",
                                quad[0] + base,
                                quad[1] + base,
                                quad[2] + base,
                                quad[3] + base
                            )?;
                        }
                    }
                    // A ring of any length - OBJ's `f` takes as many indices as
                    // it is given, so an outline goes in as one face.
                    Topology::Polygons(rings) => {
                        for ring in rings {
                            let nodes: Vec<String> =
                                ring.iter().map(|node| (node + base).to_string()).collect();
                            writeln!(out, "f {}", nodes.join(" "))?;
                        }
                    }
                    // A lattice still asked for as a lattice, and a point set.
                    // `Shape::Surface` - the default for a grid - turns the
                    // first into `Quads` above.
                    Topology::Grid { .. } | Topology::Points => {
                        writeln!(out, "# {} point(s), no faces", patch.points.len())?;
                    }
                }
                base += patch.points.len() as u32;
            }
        }
        Ok(())
    }
}

/// Everything a person needs to find this object again.
///
/// OBJ has no metadata section, so a `#` line is the only place the uuid can
/// live where every reader ignores it and every person sees it.
fn header(
    object: &SceneObject,
    placement: Placement,
    options: &Options<'_>,
    out: &mut dyn Write,
) -> Result<()> {
    writeln!(out, "# {}", object.title)?;
    writeln!(out, "# uuid {}", object.uuid)?;
    writeln!(out, "# uri {}", object.uri)?;
    writeln!(out, "# type {}", object.qualified_type)?;
    if let Some(interpretation) = &object.interpretation {
        writeln!(
            out,
            "# interpretation {} {}",
            interpretation.uuid, interpretation.title
        )?;
    }
    if let Some(feature) = &object.feature {
        writeln!(out, "# feature {} {}", feature.uuid, feature.title)?;
    }
    // The caller's `--metadata` spec, one comment line each. Only what was
    // found: a comment naming a key with nothing after it tells a reader
    // nothing they could act on, and OBJ has no null to mean "asked for and
    // absent" the way a GeoJSON property does.
    for (key, value) in object.metadata.pairs() {
        writeln!(out, "# {key} {value}")?;
    }
    match (&object.crs, placement) {
        (Some(crs), Placement::Local) => writeln!(out, "# crs {} (local coordinates)", crs.uuid)?,
        (Some(crs), _) => {
            let system = match crs.projected.epsg() {
                Some(code) => format!("EPSG:{code}"),
                None => format!("{:?}", crs.projected),
            };
            // *"metres, z up"* would be a lie on a time axis, and the sort of
            // lie a reader has no way to catch: the numbers look like depths.
            let vertical = if crs.is_time() {
                "z from travel time"
            } else {
                "metres, z up"
            };
            writeln!(out, "# crs {} {system}, {vertical}", crs.uuid)?;
        }
        (None, _) => writeln!(out, "# crs none stated; coordinates are as written")?,
    }
    // The equivalence, beside the geometry it produced. A mesh loses its
    // context the day it is exported; this is the last place anything can still
    // say what the Z means.
    if let Some(note) = options.time_equivalence_note(object) {
        for line in wrap_comment(&note) {
            writeln!(out, "# {line}")?;
        }
    }
    Ok(())
}

/// The six faces of a hexahedron in VTK corner order, each wound so its normal
/// points out of the cell.
const HEXAHEDRON_FACES: [[usize; 4]; 6] = [
    [0, 3, 2, 1], // the 0-1-2-3 face, reversed so the normal points outward
    [4, 5, 6, 7],
    [0, 1, 5, 4],
    [1, 2, 6, 5],
    [2, 3, 7, 6],
    [3, 0, 4, 7],
];

/// An OBJ object name: no spaces, and never empty.
fn sanitise(title: &str, uuid: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .filter(|c| !matches!(c, '#' | '\n' | '\r'))
        .collect();
    if cleaned.trim().is_empty() {
        uuid.to_string()
    } else {
        cleaned
    }
}

/// Break a sentence into comment-length lines.
///
/// An OBJ comment has no continuation and no length limit, but a 300-character
/// `#` line is one a person scrolls past - and this one is the caveat that
/// stops them measuring a depth off the file, so it has to be readable.
fn wrap_comment(text: &str) -> Vec<String> {
    const WIDTH: usize = 76;
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > WIDTH {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
