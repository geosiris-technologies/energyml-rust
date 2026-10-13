//! VTK, in the three flavours a caller is likely to be asked for.
//!
//! | name | file | what it is |
//! |---|---|---|
//! | `vtk` | `.vtk` | the **legacy** ASCII format, `DATASET POLYDATA` |
//! | `vtp` | `.vtp` | the **XML** format, `PolyData` |
//! | `vtu` | `.vtu` | the **XML** format, `UnstructuredGrid` |
//!
//! Three rather than one because they are not interchangeable in practice. The
//! legacy format is read by everything, including old and in-house tools, and
//! is the one to hand somebody who just needs it to open. The XML formats are
//! what ParaView writes and what carries metadata properly. `PolyData` suits
//! surfaces and lines; `UnstructuredGrid` is the general container, and the one
//! to grow into for volumes.
//!
//! Each XML flavour takes a **file-format version** - VTK's own `version`
//! attribute, not the VTK library's. `0.1` is what VTK 5-8 wrote, `1.0` is the
//! long-standing default, `2.2` adds 64-bit headers. A reader older than the
//! version it is handed refuses the file, so the default here is `1.0`: the one
//! nothing refuses.
//!
//! # Metadata
//!
//! Every flavour carries whatever the [`Spec`] collected.
//!
//! - Legacy: the title line, then `FIELD FieldData` with one `string` array per
//!   key. String arrays in legacy VTK are a real feature and an obscure one;
//!   the comment lines above them are what a person actually reads.
//! - XML: a `<FieldData>` block of `type="String"` arrays. VTK encodes a string
//!   array as the **character codes** of each string followed by a `0`
//!   terminator, which looks wrong and is not. An XML comment repeats the pairs
//!   in plain text, because a format whose metadata only a library can read is
//!   half a format.
//!
//! Neither encoding has been round-tripped through ParaView in this
//! repository - there is none here to run - so they follow the documented
//! format rather than an observed one. Worth checking against a real reader
//! before relying on the machine-readable half.
//!
//! [`Spec`]: energyml_domain::metadata::Spec

use std::io::Write;

use energyml_geometry::{Attribute, AttributeScope, Placement, SceneObject, Topology};

use crate::error::{Error, Result};
use crate::registry::{Exporter, Options};

/// Which VTK container to write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavour {
    /// Legacy ASCII `.vtk`, `DATASET POLYDATA`.
    Legacy,
    /// XML `.vtp`, `PolyData`.
    PolyData,
    /// XML `.vtu`, `UnstructuredGrid`.
    UnstructuredGrid,
}

/// Writes VTK.
pub struct VtkExporter {
    flavour: Flavour,
    version: &'static str,
}

impl VtkExporter {
    /// The legacy ASCII format.
    pub fn legacy() -> Self {
        Self {
            flavour: Flavour::Legacy,
            // Legacy files declare `# vtk DataFile Version x.y`. 3.0 is what
            // every reader since VTK 4 accepts; 5.1 adds nothing this exporter
            // uses and excludes older tools.
            version: "3.0",
        }
    }

    /// XML `PolyData`, at the default file-format version.
    pub fn poly_data() -> Self {
        Self {
            flavour: Flavour::PolyData,
            version: "1.0",
        }
    }

    /// XML `UnstructuredGrid`, at the default file-format version.
    pub fn unstructured_grid() -> Self {
        Self {
            flavour: Flavour::UnstructuredGrid,
            version: "1.0",
        }
    }

    /// Set the file-format version written into the header.
    ///
    /// `"0.1"`, `"1.0"` or `"2.2"` for the XML flavours; `"2.0"`, `"3.0"` or
    /// `"5.1"` for the legacy one. Nothing else about the output changes - this
    /// is what the file *claims to be*, and claiming a version a reader does
    /// not know makes it refuse the file.
    pub fn with_version(mut self, version: &'static str) -> Self {
        self.version = version;
        self
    }

    /// Which container this writes.
    pub fn flavour(&self) -> Flavour {
        self.flavour
    }
}

impl Exporter for VtkExporter {
    fn name(&self) -> &'static str {
        match self.flavour {
            Flavour::Legacy => "vtk",
            Flavour::PolyData => "vtp",
            Flavour::UnstructuredGrid => "vtu",
        }
    }

    fn extension(&self) -> &'static str {
        self.name()
    }

    fn mime(&self) -> &'static str {
        match self.flavour {
            Flavour::Legacy => "application/octet-stream",
            _ => "application/xml",
        }
    }

    fn describe(&self) -> &'static str {
        match self.flavour {
            Flavour::Legacy => "VTK legacy ASCII, DATASET POLYDATA",
            Flavour::PolyData => "VTK XML PolyData (.vtp), surfaces and lines",
            Flavour::UnstructuredGrid => "VTK XML UnstructuredGrid (.vtu), the general container",
        }
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
                "vtk",
                format!("`{}`, which has no points", object.title),
            ));
        }
        // Reshaped as the caller's rules ask, then stripped of the nodes that
        // have no position - one funnel, one order, for every format. A `nan`
        // coordinate lands a point at the origin in some VTK readers and stops
        // the file loading in others; property values follow their elements.
        // See `registry::prepare`. (Coordinates only: a `nan` in a *property*
        // array is deliberate and stays - VTK reads it as the no-data it is.)
        let object = crate::registry::prepare(object, options)?;
        let object = object.as_ref();

        let placement = options.placement.unwrap_or_else(|| self.placement());
        let points = options.points(object, placement)?;
        let cells = cells_of(object);

        match self.flavour {
            Flavour::Legacy => self.legacy_document(object, &points, &cells, options, out),
            _ => self.xml_document(object, &points, &cells, options, out),
        }
    }

    // One object per file, deliberately. VTK's legacy format holds one dataset,
    // and the XML ones hold one piece per file unless you write a `.pvd`
    // collection - which is a different file referencing these. Concatenating
    // would produce something no reader accepts, so this refuses instead.
    fn write_all(&self, objects: &[SceneObject], out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
        let writable: Vec<&SceneObject> = objects.iter().filter(|o| self.handles(o)).collect();
        match writable.len() {
            0 => Ok(()),
            1 => self.write(writable[0], out, options),
            n => Err(Error::unsuitable(
                "vtk",
                format!(
                    "{n} objects in one file - a VTK file holds one dataset; \
                     write to a directory to get one file each"
                ),
            )),
        }
    }
}

/// The cells of an object, as VTK counts them.
struct Cells {
    /// Triangles, as three point indices.
    triangles: Vec<[u32; 3]>,
    /// Rings of any length: `VTK_POLYGON`, for an outline and anything else a
    /// fixed corner count cannot hold.
    polygons: Vec<Vec<u32>>,
    /// Quadrilaterals, as four point indices in ring order.
    ///
    /// A 2D grid's cells, and the faces of a hexahedral grid asked for as a
    /// surface. `VTK_QUAD` rather than two `VTK_TRIANGLE`s, because that is
    /// what the file describes and ParaView's cell count should say what the
    /// grid's does.
    quads: Vec<[u32; 4]>,
    /// Polylines, as runs of consecutive point indices.
    lines: Vec<Vec<u32>>,
    /// Points that form nothing.
    vertices: Vec<u32>,
    /// Cells of eight corners, in VTK's order.
    hexahedra: Vec<[u32; 8]>,
    /// Cells of arbitrary faces: one entry per cell, one inner list per face,
    /// each already wound so its normal points **out** of that cell.
    ///
    /// VTK calls this `VTK_POLYHEDRON` and encodes it unlike every other cell -
    /// see [`write_unstructured_cells`], where it costs two extra arrays.
    polyhedra: Vec<Vec<Vec<u32>>>,
}

impl Cells {
    /// How many cells of every kind, in the order the writers emit them.
    ///
    /// One number used by three writers, so the `types` array and the
    /// per-cell `faceoffsets` cannot drift apart from the connectivity.
    fn count(&self) -> usize {
        self.vertices.len()
            + self.lines.len()
            + self.triangles.len()
            + self.quads.len()
            + self.polygons.len()
            + self.hexahedra.len()
            + self.polyhedra.len()
    }
}

/// The six faces of a hexahedron in VTK corner order, wound outward.
///
/// Needed only by `PolyData`, which has no volume cells: a grid written as
/// `.vtp` becomes its faces, interior ones included. `.vtu` holds the cells
/// themselves and is the right container for a grid.
const HEXAHEDRON_FACES: [[usize; 4]; 6] = [
    [0, 3, 2, 1],
    [4, 5, 6, 7],
    [0, 1, 5, 4],
    [1, 2, 6, 5],
    [2, 3, 7, 6],
    [3, 0, 4, 7],
];

fn cells_of(object: &SceneObject) -> Cells {
    let (_, triangles) = object.geometry.merged();
    let mut lines = Vec::new();
    let mut quads = Vec::new();
    let mut polygons: Vec<Vec<u32>> = Vec::new();
    let mut vertices = Vec::new();
    let mut hexahedra = Vec::new();
    let mut polyhedra: Vec<Vec<Vec<u32>>> = Vec::new();
    let mut base = 0u32;
    for patch in &object.geometry.patches {
        match &patch.topology {
            // A wireframe's edge is a two-point line; VTK's polyline cell
            // takes any indices, consecutive or not.
            Topology::Segments(pairs) => {
                lines.extend(pairs.iter().map(|pair| vec![pair[0] + base, pair[1] + base]));
            }
            Topology::Polylines(runs) => {
                let mut start = base;
                for (rank, run) in runs.iter().enumerate() {
                    if *run >= 2 {
                        let mut line: Vec<_> = (0..*run).map(|i| start + i).collect();
                        // A VTK polyline cell has no closing flag either: a
                        // ring the file calls closed ends on its first point.
                        if patch.closed.get(rank) == Some(&true) {
                            line.push(start);
                        }
                        lines.push(line);
                    }
                    start += run;
                }
            }
            // Already wound by `energyml_geometry::shape`, which measures the
            // signed area rather than assuming a convention the file does not
            // state - the reason a grid can be drawn as cells at all.
            Topology::Quads(quadrilaterals) => {
                quads.extend(quadrilaterals.iter().map(|quad| {
                    let mut shifted = *quad;
                    for corner in &mut shifted {
                        *corner += base;
                    }
                    shifted
                }));
            }
            Topology::Polygons(rings) => {
                polygons.extend(
                    rings
                        .iter()
                        .map(|ring| ring.iter().map(|node| node + base).collect()),
                );
            }
            // A lattice still asked for as a lattice, and a point set: both are
            // loose points here. `Shape::Surface` (the default for a grid) turns
            // the first into `Quads` above.
            Topology::Points | Topology::Grid { .. } => {
                vertices.extend(base..base + patch.points.len() as u32);
            }
            Topology::Hexahedra { cells, .. } => {
                hexahedra.extend(cells.iter().map(|cell| {
                    let mut shifted = *cell;
                    for corner in &mut shifted {
                        *corner += base;
                    }
                    shifted
                }));
            }
            // Already oriented outward by `faces_outward`, so nothing here has
            // to know about `CellFaceIsRightHanded`.
            Topology::Polyhedra(cells) => {
                polyhedra.extend((0..cells.cell_count()).map(|cell| {
                    cells
                        .faces_outward(cell)
                        .map(|face| face.iter().map(|node| node + base).collect())
                        .collect()
                }));
            }
            Topology::Triangles(_) => {}
        }
        base += patch.points.len() as u32;
    }
    Cells {
        triangles,
        quads,
        polygons,
        lines,
        vertices,
        hexahedra,
        polyhedra,
    }
}

impl VtkExporter {
    fn legacy_document(
        &self,
        object: &SceneObject,
        points: &[[f64; 3]],
        cells: &Cells,
        options: &Options<'_>,
        out: &mut dyn Write,
    ) -> Result<()> {
        writeln!(out, "# vtk DataFile Version {}", self.version)?;
        // The title line is capped at 256 characters by the format, so it holds
        // the two things worth having and the rest goes to FieldData.
        let title = format!("{} [{}]", one_line(&object.title), object.uuid);
        writeln!(out, "{}", &title[..title.len().min(255)])?;
        writeln!(out, "ASCII")?;
        // A legacy file states one dataset kind up front, and `POLYDATA` has no
        // volume cells at all. A grid therefore has to be written as an
        // unstructured grid - the same points, a different envelope.
        let volumetric = !cells.hexahedra.is_empty() || !cells.polyhedra.is_empty();
        writeln!(
            out,
            "DATASET {}",
            if volumetric {
                "UNSTRUCTURED_GRID"
            } else {
                "POLYDATA"
            }
        )?;

        writeln!(out, "POINTS {} double", points.len())?;
        for point in points {
            writeln!(out, "{} {} {}", point[0], point[1], point[2])?;
        }
        if volumetric {
            // `CELLS n size` where size counts the leading count of each cell.
            let count = cells.hexahedra.len()
                + cells.polyhedra.len()
                + cells.triangles.len()
                + cells.quads.len()
                + cells.polygons.len()
                + cells.lines.len();
            let size: usize = cells.hexahedra.len() * 9
                + cells
                    .polyhedra
                    .iter()
                    .map(|c| legacy_polyhedron_len(c) + 1)
                    .sum::<usize>()
                + cells.triangles.len() * 4
                + cells.quads.len() * 5
                + cells.polygons.iter().map(|p| p.len() + 1).sum::<usize>()
                + cells.lines.iter().map(|l| l.len() + 1).sum::<usize>();
            writeln!(out, "CELLS {count} {size}")?;
            for cell in &cells.hexahedra {
                let corners: Vec<String> = cell.iter().map(|c| c.to_string()).collect();
                writeln!(out, "8 {}", corners.join(" "))?;
            }
            // A polyhedron's line is `n` then the face stream, where `n` is
            // how many integers follow - not how many points, which is what
            // every other cell type puts there.
            for cell in &cells.polyhedra {
                let mut stream = vec![cell.len().to_string()];
                for face in cell {
                    stream.push(face.len().to_string());
                    stream.extend(face.iter().map(|node| node.to_string()));
                }
                writeln!(out, "{} {}", legacy_polyhedron_len(cell), stream.join(" "))?;
            }
            for triangle in &cells.triangles {
                writeln!(out, "3 {} {} {}", triangle[0], triangle[1], triangle[2])?;
            }
            for quad in &cells.quads {
                writeln!(out, "4 {} {} {} {}", quad[0], quad[1], quad[2], quad[3])?;
            }
            for ring in &cells.polygons {
                let nodes: Vec<String> = ring.iter().map(|n| n.to_string()).collect();
                writeln!(out, "{} {}", ring.len(), nodes.join(" "))?;
            }
            for line in &cells.lines {
                let indices: Vec<String> = line.iter().map(|i| i.to_string()).collect();
                writeln!(out, "{} {}", line.len(), indices.join(" "))?;
            }
            writeln!(out, "CELL_TYPES {count}")?;
            for _ in &cells.hexahedra {
                writeln!(out, "12")?; // VTK_HEXAHEDRON
            }
            for _ in &cells.polyhedra {
                writeln!(out, "42")?; // VTK_POLYHEDRON
            }
            for _ in &cells.triangles {
                writeln!(out, "5")?; // VTK_TRIANGLE
            }
            for _ in &cells.quads {
                writeln!(out, "9")?; // VTK_QUAD
            }
            for _ in &cells.polygons {
                writeln!(out, "7")?; // VTK_POLYGON
            }
            for _ in &cells.lines {
                writeln!(out, "4")?; // VTK_POLY_LINE
            }
        }
        if !volumetric
            && (!cells.triangles.is_empty() || !cells.quads.is_empty() || !cells.polygons.is_empty())
        {
            let size: usize = cells.triangles.len() * 4
                + cells.quads.len() * 5
                + cells.polygons.iter().map(|p| p.len() + 1).sum::<usize>();
            let count = cells.triangles.len() + cells.quads.len() + cells.polygons.len();
            writeln!(out, "POLYGONS {count} {size}")?;
            for triangle in &cells.triangles {
                writeln!(out, "3 {} {} {}", triangle[0], triangle[1], triangle[2])?;
            }
            for quad in &cells.quads {
                writeln!(out, "4 {} {} {} {}", quad[0], quad[1], quad[2], quad[3])?;
            }
            for ring in &cells.polygons {
                let nodes: Vec<String> = ring.iter().map(|n| n.to_string()).collect();
                writeln!(out, "{} {}", ring.len(), nodes.join(" "))?;
            }
        }
        if !volumetric && !cells.lines.is_empty() {
            let size: usize = cells.lines.iter().map(|l| l.len() + 1).sum();
            writeln!(out, "LINES {} {size}", cells.lines.len())?;
            for line in &cells.lines {
                let indices: Vec<String> = line.iter().map(|i| i.to_string()).collect();
                writeln!(out, "{} {}", line.len(), indices.join(" "))?;
            }
        }
        if !cells.vertices.is_empty() {
            writeln!(
                out,
                "VERTICES {} {}",
                cells.vertices.len(),
                cells.vertices.len() * 2
            )?;
            for vertex in &cells.vertices {
                writeln!(out, "1 {vertex}")?;
            }
        }

        let pairs = self.pairs(object, options);
        if !pairs.is_empty() {
            writeln!(out, "FIELD FieldData {}", pairs.len())?;
            for (key, value) in &pairs {
                // A legacy string array: name, 1 component, 1 tuple, type
                // `string`, then the value on the next line.
                writeln!(out, "{} 1 1 string", sanitise(key))?;
                writeln!(out, "{}", one_line(value))?;
            }
        }

        // The property values. `POINT_DATA`/`CELL_DATA` each open a section
        // that everything after belongs to, so each is written once with all
        // its arrays - writing the header twice makes VTK read the second set
        // as replacing the first.
        let cell_count = cells.count();
        for (scope, count) in [
            (AttributeScope::Points, points.len()),
            (AttributeScope::Cells, cell_count),
        ] {
            let attributes: Vec<&Attribute> = object
                .attributes
                .iter()
                .filter(|attribute| attribute.scope == scope)
                .collect();
            if attributes.is_empty() || count == 0 {
                continue;
            }
            writeln!(
                out,
                "{} {count}",
                match scope {
                    AttributeScope::Points => "POINT_DATA",
                    AttributeScope::Cells => "CELL_DATA",
                }
            )?;
            for attribute in attributes {
                // `SCALARS name double n` then `LOOKUP_TABLE`, which is what
                // makes ParaView offer the array in its colour-by list. When
                // the package states a colour map for the property, its ramp
                // goes in beside the values under a name of its own; otherwise
                // `default` leaves the choice to the viewer rather than
                // inventing a ramp.
                let table = if attribute.lut.is_empty() {
                    "default".to_string()
                } else {
                    format!("{}_colors", sanitise(&attribute.name))
                };
                writeln!(
                    out,
                    "SCALARS {} double {}",
                    sanitise(&attribute.name),
                    attribute.components.max(1)
                )?;
                writeln!(out, "LOOKUP_TABLE {table}")?;
                for chunk in attribute.values.chunks(attribute.components.max(1) as usize) {
                    let numbers: Vec<String> = chunk.iter().map(number).collect();
                    writeln!(out, "{}", numbers.join(" "))?;
                }
                if !attribute.lut.is_empty() {
                    // The legacy format is the only VTK file that carries a
                    // colour table at all: `.vtu` and `.vtp` have no element
                    // for one, and ParaView keeps ramps in its own state.
                    writeln!(out, "LOOKUP_TABLE {table} {}", attribute.lut.len())?;
                    for [r, g, b, a] in &attribute.lut {
                        writeln!(out, "{r} {g} {b} {a}")?;
                    }
                }
            }
        }
        Ok(())
    }

    fn xml_document(
        &self,
        object: &SceneObject,
        points: &[[f64; 3]],
        cells: &Cells,
        options: &Options<'_>,
        out: &mut dyn Write,
    ) -> Result<()> {
        let container = match self.flavour {
            Flavour::UnstructuredGrid => "UnstructuredGrid",
            _ => "PolyData",
        };
        writeln!(out, "<?xml version=\"1.0\"?>")?;
        writeln!(
            out,
            "<VTKFile type=\"{container}\" version=\"{}\" byte_order=\"LittleEndian\">",
            self.version
        )?;

        // Plain text first: a person opening the file should not need a library
        // to see what it is.
        let pairs = self.pairs(object, options);
        if !pairs.is_empty() {
            writeln!(out, "  <!--")?;
            for (key, value) in &pairs {
                writeln!(out, "    {key}: {}", one_line(value).replace("--", "- -"))?;
            }
            writeln!(out, "  -->")?;
        }

        writeln!(out, "  <{container}>")?;
        if !pairs.is_empty() {
            writeln!(out, "    <FieldData>")?;
            for (key, value) in &pairs {
                let codes: Vec<String> = one_line(value)
                    .chars()
                    .map(|c| (c as u32).to_string())
                    .chain(std::iter::once("0".to_string()))
                    .collect();
                writeln!(
                    out,
                    "      <DataArray type=\"String\" Name=\"{}\" NumberOfTuples=\"1\" format=\"ascii\">",
                    escape(&sanitise(key))
                )?;
                writeln!(out, "        {}", codes.join(" "))?;
                writeln!(out, "      </DataArray>")?;
            }
            writeln!(out, "    </FieldData>")?;
        }

        match self.flavour {
            Flavour::UnstructuredGrid => {
                let count = cells.count();
                writeln!(
                    out,
                    "    <Piece NumberOfPoints=\"{}\" NumberOfCells=\"{count}\">",
                    points.len()
                )?;
                write_points(points, out)?;
                write_unstructured_cells(cells, out)?;
            }
            _ => {
                writeln!(
                    out,
                    "    <Piece NumberOfPoints=\"{}\" NumberOfVerts=\"{}\" NumberOfLines=\"{}\" \
                     NumberOfStrips=\"0\" NumberOfPolys=\"{}\">",
                    points.len(),
                    cells.vertices.len(),
                    cells.lines.len(),
                    // PolyData has no volume cells, so a grid contributes its
                    // faces instead - six per hexahedron, and however many an
                    // unstructured cell happens to have.
                    cells.triangles.len()
                        + cells.quads.len()
                        + cells.polygons.len()
                        + cells.hexahedra.len() * 6
                        + cells.polyhedra.iter().map(|cell| cell.len()).sum::<usize>()
                )?;
                write_points(points, out)?;
                write_poly_cells(cells, out)?;
            }
        }
        write_attributes(object, cells, out)?;
        writeln!(out, "    </Piece>")?;
        writeln!(out, "  </{container}>")?;
        writeln!(out, "</VTKFile>")?;
        Ok(())
    }

    /// The metadata, with the identity always present.
    ///
    /// A spec may be empty or may not have been given; the uuid and the type
    /// go in regardless, because a VTK file that cannot be traced back to the
    /// object it came from is the thing this whole layer exists to avoid.
    fn pairs(&self, object: &SceneObject, options: &Options<'_>) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = vec![
            ("uuid".to_string(), object.uuid.clone()),
            ("uri".to_string(), object.uri.clone()),
            ("title".to_string(), object.title.clone()),
            ("type".to_string(), object.qualified_type.clone()),
        ];
        if let Some(crs) = &object.crs {
            let system = match crs.projected.epsg() {
                Some(code) => format!("EPSG:{code}"),
                None => format!("{:?}", crs.projected),
            };
            pairs.push(("crs".to_string(), format!("{} {system}", crs.uuid)));
        }
        // The equivalence, in the file's own metadata block. A VTK travels
        // between tools that know nothing of RESQML; this is the only place the
        // Z's meaning can still be read once it has.
        if let Some(note) = options.time_equivalence_note(object) {
            pairs.push(("vertical_axis".to_string(), note));
        }
        for (key, value) in object.metadata.pairs() {
            // What the spec says wins: it is the caller's explicit choice.
            if let Some(existing) = pairs.iter_mut().find(|(k, _)| k == key) {
                existing.1 = value.to_string();
            } else {
                pairs.push((key.to_string(), value.to_string()));
            }
        }
        pairs.retain(|(_, value)| !value.is_empty());
        pairs
    }
}

/// `<PointData>` and `<CellData>` blocks, one array each.
///
/// `Scalars="…"` on the block names the array a viewer colours by when it
/// opens the file - without it ParaView shows the geometry in a flat colour
/// and the values have to be found by hand.
fn write_attributes(object: &SceneObject, cells: &Cells, out: &mut dyn Write) -> Result<()> {
    for (scope, tag) in [
        (AttributeScope::Points, "PointData"),
        (AttributeScope::Cells, "CellData"),
    ] {
        let attributes: Vec<&Attribute> = object
            .attributes
            .iter()
            .filter(|attribute| attribute.scope == scope)
            .collect();
        if attributes.is_empty() {
            continue;
        }
        let _ = cells;
        writeln!(
            out,
            "      <{tag} Scalars=\"{}\">",
            escape(&sanitise(&attributes[0].name))
        )?;
        for attribute in attributes {
            writeln!(
                out,
                "        <DataArray type=\"Float64\" Name=\"{}\" NumberOfComponents=\"{}\" \
                 format=\"ascii\">",
                escape(&sanitise(&attribute.name)),
                attribute.components.max(1)
            )?;
            let numbers: Vec<String> = attribute.values.iter().map(number).collect();
            writeln!(out, "          {}", numbers.join(" "))?;
            writeln!(out, "        </DataArray>")?;
        }
        writeln!(out, "      </{tag}>")?;
    }
    Ok(())
}

/// A number VTK will read back as what was written.
///
/// `NaN` and the infinities have no lexical form in a VTK ASCII file; a
/// property that carries them writes `nan`/`inf`, which VTK reads as such.
fn number(value: &f64) -> String {
    if value.is_nan() {
        "nan".to_string()
    } else if value.is_infinite() {
        if *value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        }
    } else {
        value.to_string()
    }
}

fn write_points(points: &[[f64; 3]], out: &mut dyn Write) -> Result<()> {
    writeln!(out, "      <Points>")?;
    writeln!(
        out,
        "        <DataArray type=\"Float64\" NumberOfComponents=\"3\" format=\"ascii\">"
    )?;
    for point in points {
        writeln!(out, "          {} {} {}", point[0], point[1], point[2])?;
    }
    writeln!(out, "        </DataArray>")?;
    writeln!(out, "      </Points>")?;
    Ok(())
}

/// How many integers a legacy `VTK_POLYHEDRON` face stream occupies.
///
/// `numFaces`, then per face its point count and its points. This is the number
/// the legacy format wants as the cell's leading value - for every *other* cell
/// type that leading value is the point count, which is why it is computed by a
/// named function rather than inline.
fn legacy_polyhedron_len(cell: &[Vec<u32>]) -> usize {
    1 + cell.iter().map(|face| face.len() + 1).sum::<usize>()
}

fn write_poly_cells(cells: &Cells, out: &mut dyn Write) -> Result<()> {
    if !cells.vertices.is_empty() {
        let connectivity: Vec<String> = cells.vertices.iter().map(|v| v.to_string()).collect();
        let offsets: Vec<String> = (1..=cells.vertices.len()).map(|i| i.to_string()).collect();
        section("Verts", &connectivity, &offsets, out)?;
    }
    if !cells.lines.is_empty() {
        let mut connectivity = Vec::new();
        let mut offsets = Vec::new();
        let mut at = 0usize;
        for line in &cells.lines {
            connectivity.extend(line.iter().map(|i| i.to_string()));
            at += line.len();
            offsets.push(at.to_string());
        }
        section("Lines", &connectivity, &offsets, out)?;
    }
    if !cells.triangles.is_empty()
        || !cells.quads.is_empty()
        || !cells.polygons.is_empty()
        || !cells.hexahedra.is_empty()
        || !cells.polyhedra.is_empty()
    {
        let mut connectivity = Vec::new();
        let mut offsets = Vec::new();
        let mut at = 0usize;
        for triangle in &cells.triangles {
            connectivity.extend(triangle.iter().map(|v| v.to_string()));
            at += 3;
            offsets.push(at.to_string());
        }
        for quad in &cells.quads {
            connectivity.extend(quad.iter().map(|v| v.to_string()));
            at += 4;
            offsets.push(at.to_string());
        }
        for ring in &cells.polygons {
            connectivity.extend(ring.iter().map(|v| v.to_string()));
            at += ring.len();
            offsets.push(at.to_string());
        }
        for cell in &cells.hexahedra {
            for face in HEXAHEDRON_FACES {
                connectivity.extend(face.iter().map(|corner| cell[*corner].to_string()));
                at += 4;
                offsets.push(at.to_string());
            }
        }
        // `Polys` takes a polygon of any size, so an unstructured cell's faces
        // go in whole rather than fan-triangulated. A quadrilateral stays a
        // quadrilateral, which is what the file said.
        for cell in &cells.polyhedra {
            for face in cell {
                connectivity.extend(face.iter().map(|node| node.to_string()));
                at += face.len();
                offsets.push(at.to_string());
            }
        }
        section("Polys", &connectivity, &offsets, out)?;
    }
    Ok(())
}

fn write_unstructured_cells(cells: &Cells, out: &mut dyn Write) -> Result<()> {
    let mut connectivity: Vec<String> = Vec::new();
    let mut offsets: Vec<String> = Vec::new();
    let mut types: Vec<String> = Vec::new();
    let mut at = 0usize;

    // VTK cell type numbers: 1 = vertex, 4 = polyline, 5 = triangle,
    // 7 = polygon, 9 = quad, 12 = hexahedron.
    for vertex in &cells.vertices {
        connectivity.push(vertex.to_string());
        at += 1;
        offsets.push(at.to_string());
        types.push("1".to_string());
    }
    for line in &cells.lines {
        connectivity.extend(line.iter().map(|i| i.to_string()));
        at += line.len();
        offsets.push(at.to_string());
        types.push("4".to_string());
    }
    for triangle in &cells.triangles {
        connectivity.extend(triangle.iter().map(|v| v.to_string()));
        at += 3;
        offsets.push(at.to_string());
        types.push("5".to_string());
    }
    for quad in &cells.quads {
        connectivity.extend(quad.iter().map(|v| v.to_string()));
        at += 4;
        offsets.push(at.to_string());
        types.push("9".to_string());
    }
    for ring in &cells.polygons {
        connectivity.extend(ring.iter().map(|v| v.to_string()));
        at += ring.len();
        offsets.push(at.to_string());
        types.push("7".to_string());
    }
    for cell in &cells.hexahedra {
        connectivity.extend(cell.iter().map(|corner| corner.to_string()));
        at += 8;
        offsets.push(at.to_string());
        types.push("12".to_string());
    }

    // `VTK_POLYHEDRON` is the one cell type whose connectivity does not
    // describe it. The connectivity holds the cell's points, in no particular
    // order and each **once**; the faces live in two further arrays that only
    // exist when a polyhedron is present. `faceoffsets` carries one entry per
    // cell of *every* kind, with −1 for the ones that are not polyhedra, so the
    // vertices, lines, triangles and hexahedra written above have to be
    // accounted for even though they contribute nothing to `faces`.
    let mut faces: Vec<String> = Vec::new();
    let mut face_offsets: Vec<String> = Vec::new();
    for cell in &cells.polyhedra {
        let mut points: Vec<u32> = cell.iter().flatten().copied().collect();
        points.sort_unstable();
        points.dedup();
        connectivity.extend(points.iter().map(|node| node.to_string()));
        at += points.len();
        offsets.push(at.to_string());
        types.push("42".to_string());

        faces.push(cell.len().to_string());
        for face in cell {
            faces.push(face.len().to_string());
            faces.extend(face.iter().map(|node| node.to_string()));
        }
        face_offsets.push(faces.len().to_string());
    }

    writeln!(out, "      <Cells>")?;
    array("Int64", "connectivity", &connectivity, out)?;
    array("Int64", "offsets", &offsets, out)?;
    array("UInt8", "types", &types, out)?;
    if !faces.is_empty() {
        let others = cells.count() - cells.polyhedra.len();
        let mut all: Vec<String> = vec!["-1".to_string(); others];
        all.extend(face_offsets);
        array("Int64", "faces", &faces, out)?;
        array("Int64", "faceoffsets", &all, out)?;
    }
    writeln!(out, "      </Cells>")?;
    Ok(())
}

fn section(name: &str, connectivity: &[String], offsets: &[String], out: &mut dyn Write) -> Result<()> {
    writeln!(out, "      <{name}>")?;
    array("Int64", "connectivity", connectivity, out)?;
    array("Int64", "offsets", offsets, out)?;
    writeln!(out, "      </{name}>")?;
    Ok(())
}

fn array(kind: &str, name: &str, values: &[String], out: &mut dyn Write) -> Result<()> {
    writeln!(
        out,
        "        <DataArray type=\"{kind}\" Name=\"{name}\" format=\"ascii\">"
    )?;
    writeln!(out, "          {}", values.join(" "))?;
    writeln!(out, "        </DataArray>")?;
    Ok(())
}

/// VTK array names cannot hold whitespace.
fn sanitise(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect();
    if cleaned.is_empty() {
        "field".to_string()
    } else {
        cleaned
    }
}

/// A value on one line: legacy VTK is line-oriented, and a newline inside a
/// field would end the record early.
fn one_line(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
