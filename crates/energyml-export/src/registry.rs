//! The exporter trait and the registry that holds them.

use std::io::Write;

use energyml_domain::reproject::Reprojector;
use energyml_geometry::{Placement, Point, SceneObject};

use crate::error::{Error, Result};

/// What a caller can ask of an export, beyond the object itself.
#[derive(Default)]
pub struct Options<'a> {
    /// A geodesy backend, for the formats that need real coordinates.
    ///
    /// `None` is a supported state: an exporter that needs one says so by name
    /// rather than writing projected metres into a longitude field.
    pub reprojector: Option<&'a dyn Reprojector>,
    /// Override the placement the exporter would choose.
    ///
    /// For a caller that knows better — a viewer that wants local coordinates
    /// out of a format that would normally be projected.
    pub placement: Option<Placement>,
    /// Reproject into this EPSG system before writing.
    ///
    /// Overrides `placement`, which stops meaning anything once a datum is
    /// crossed: reprojection starts from placed metres or it starts from
    /// nowhere.
    pub target_epsg: Option<u64>,
    /// Write coordinates as they are stored, reprojecting nothing.
    ///
    /// The escape hatch for the formats whose specification fixes a coordinate
    /// system — GeoJSON is WGS 84 by definition. With this set, such a format
    /// writes the **placed native coordinates** instead, embeds the legacy
    /// 2008 `crs` member when the EPSG code is known (QGIS and GDAL honour
    /// it), and accepts objects that name no CRS at all.
    ///
    /// The file is then knowingly non-conformant. That is the point: for a
    /// package under repair, coordinates one can look at beat a refusal — but
    /// it has to be asked for, because a conformant-looking file with the
    /// wrong coordinate system is the silent failure this crate is arranged
    /// against.
    pub native_crs: bool,
    /// Which form to draw each type in — see [`crate::shapes`].
    ///
    /// Empty means every object in its natural shape, which is what every
    /// export did before there was anything to ask.
    pub shapes: crate::shapes::Shapes,
    /// **Length units one second of vertical travel time stands for.**
    ///
    /// `None` — the default — keeps the refusal: a representation in a time
    /// system cannot be written in metres, and saying so is the honest answer.
    ///
    /// # This is an equivalence, not a conversion
    ///
    /// A seismic surface has no depth. Getting one needs a velocity model the
    /// package does not carry, and RESQML puts the time *kind* — two-way
    /// against one-way — outside the CRS on purpose (TRG §12.9 leaves it to the
    /// acquisition activity), so nothing in the file even says whether to
    /// halve. Every number this produces is therefore **half or double** a real
    /// depth depending on a fact the file withholds.
    ///
    /// What it buys is a mesh: a surface that can sit in the same file as the
    /// depth objects around it, at a size somebody can look at. That is a
    /// legitimate thing to want and this is the only honest way to give it —
    /// the caller states the number, and the number is written into the file
    /// beside the geometry so the mesh carries its own caveat. A mesh loses its
    /// context the day it is exported; the export is the last moment anything
    /// can still say what it holds.
    ///
    /// Metres per second, matching the metres every exporter writes.
    pub time_equivalence: Option<f64>,
}

impl<'a> Options<'a> {
    /// Nothing special.
    pub fn new() -> Self {
        Self::default()
    }

    /// Supply a geodesy backend.
    pub fn with_reprojector(mut self, reprojector: &'a dyn Reprojector) -> Self {
        self.reprojector = Some(reprojector);
        self
    }

    /// Force a placement.
    pub fn with_placement(mut self, placement: Placement) -> Self {
        self.placement = Some(placement);
        self
    }

    /// Write coordinates in this EPSG system. Needs a backend.
    pub fn with_target_epsg(mut self, epsg: u64) -> Self {
        self.target_epsg = Some(epsg);
        self
    }

    /// Write native coordinates, reprojecting nothing. See [`Options::native_crs`].
    pub fn with_native_crs(mut self) -> Self {
        self.native_crs = true;
        self
    }

    /// Choose, per type, what form to draw. See [`crate::shapes::Shapes`].
    pub fn with_shapes(mut self, shapes: crate::shapes::Shapes) -> Self {
        self.shapes = shapes;
        self
    }

    /// State what one second of vertical travel time stands for, in metres, so
    /// that time-domain objects can be written at all.
    ///
    /// See [`Options::time_equivalence`] for what this does and does not claim.
    pub fn with_time_equivalence(mut self, metres_per_second: f64) -> Self {
        self.time_equivalence = Some(metres_per_second);
        self
    }

    /// Whether this object's vertical axis can be placed at all.
    ///
    /// `false` for a time-domain object with no equivalence stated — which is
    /// the state every export was in until one could be. Asked *before*
    /// writing, so such an object is **skipped and counted** rather than
    /// failing the whole export at the first one: nine seismic horizons in
    /// ALWYN used to abort a run that had forty perfectly writable surfaces in
    /// it, and "the export failed" is not what a person needed to be told.
    pub fn can_place_vertical(&self, object: &SceneObject) -> bool {
        let Some(crs) = object.crs.as_ref() else { return true };
        if !crs.is_time() {
            return true;
        }
        if self.time_equivalence.is_none() {
            return false;
        }
        // And the axis has to state a unit that *is* a time. A package can
        // contradict itself here — a 2.0.1 `LocalTime3dCrs` with no `TimeUom`
        // falls back to `VerticalUom`, which is a length by design — and the
        // equivalence has nothing to convert. Asked before writing so that one
        // such object is skipped and counted, instead of aborting an export of
        // forty that were fine: *"`m` is a Length unit, but a Time was expected
        // here"* is true, unactionable, and arrives after the decision to
        // export was already taken.
        match crs.vertical_axis_uom() {
            Some(unit) => energyml_domain::units::seconds(1.0, unit).is_ok(),
            // No unit stated at all is the schema's own default, and for a time
            // axis the default is the second. Taken at its word.
            None => true,
        }
    }

    /// The sentence an exporter writes into the file, and a caller prints.
    ///
    /// One place, so the comment in an OBJ, the field in a VTK and the line on
    /// a terminal cannot drift into saying different things about the same
    /// number. It names the equivalence *and* what it is not, because the
    /// second half is what stops somebody measuring a depth off the result.
    pub fn time_equivalence_note(&self, object: &SceneObject) -> Option<String> {
        let per_second = self.time_equivalence?;
        if !object.crs.as_ref().is_some_and(|crs| crs.is_time()) {
            return None;
        }
        Some(format!(
            "vertical axis is TRAVEL TIME, written with the stated equivalence \
             1 s = {per_second} m. This is a display equivalence, not a depth: \
             the velocity model is not in the package, and RESQML does not say \
             whether the time is one-way or two-way, so a real depth is half or \
             double this. Do not measure from it."
        ))
    }

    /// The points an exporter should write, placed and reprojected as asked.
    ///
    /// The single funnel every exporter goes through, so that "which
    /// coordinates am I writing" has one answer and not one per format.
    pub fn points(&self, object: &SceneObject, placement: Placement) -> Result<Vec<Point>> {
        // A time axis, before anything else: `Placement::Metres` refuses it by
        // name and `points_in_epsg` starts from metres, so both roads end at
        // the same refusal. With an equivalence stated there is something to
        // write, and it is written here rather than in each format.
        if let Some(per_second) = self.time_equivalence
            && object.crs.as_ref().is_some_and(|crs| crs.is_time())
        {
            return self.time_points(object, per_second);
        }
        match self.target_epsg {
            Some(target) => self.points_in_epsg(object, target),
            None => Ok(object.points_in(placement)?),
        }
    }

    /// A time-domain object, placed with a stated equivalence.
    ///
    /// The horizontal axes are metres like every other export — they are
    /// lengths and there is nothing special about them. Only the vertical one
    /// is converted twice: into **seconds** through the CRS's own vertical
    /// unit, which is `ms` in every corpus package and would be a silent factor
    /// of a thousand if assumed, and then into metres through the caller's
    /// equivalence.
    fn time_points(&self, object: &SceneObject, per_second: f64) -> Result<Vec<Point>> {
        let Some(crs) = object.crs.as_ref() else {
            return Err(Error::Placement(format!(
                "`{}` has a time equivalence to apply and names no CRS",
                object.title
            )));
        };
        // `Projected`, not `Metres`: the offsets and the vertical flip apply to
        // a time axis exactly as to a depth, and it is only the *unit* step
        // that has nothing to say about seconds.
        let placed = object.points_in(Placement::Projected)?;
        let horizontal = |value: f64| -> Result<f64> {
            Ok(match &crs.horizontal_uom {
                Some(unit) => energyml_domain::units::metres(value, unit)?,
                None => value,
            })
        };
        let vertical = |value: f64| -> Result<f64> {
            // `vertical_axis_uom`, never `vertical_uom`: a 2.0.1 time CRS
            // states `VerticalUom` as a **length** — it describes the ZOffset —
            // and puts the axis unit in `TimeUom`. ALWYN reads `m` for the
            // first and `ms` for the second.
            let seconds = match crs.vertical_axis_uom() {
                Some(unit) => energyml_domain::units::seconds(value, unit)?,
                // No unit stated is the schema's own default, and for a time
                // axis the default is the second. Taken at its word rather than
                // guessed at, same rule as `Crs::to_metres`.
                None => value,
            };
            Ok(seconds * per_second)
        };
        placed
            .into_iter()
            .map(|[x, y, z]| Ok([horizontal(x)?, horizontal(y)?, vertical(z)?]))
            .collect()
    }

    /// The points, in one named EPSG system.
    ///
    /// For a format whose coordinate system is fixed by its own specification
    /// — GeoJSON is WGS 84 and nothing else — rather than chosen by the caller.
    ///
    /// Placement is **always** `Metres` here, and not the caller's choice:
    /// a reprojection consumes projected metres. Feeding it local coordinates,
    /// or metres that have already been reprojected, is the kind of mistake
    /// that produces a plausible-looking file in the wrong hemisphere.
    pub fn points_in_epsg(&self, object: &SceneObject, target: u64) -> Result<Vec<Point>> {
        let placed = object.points_in(Placement::Metres)?;
        let Some(source) = object.epsg() else {
            return Err(Error::Placement(format!(
                "`{}` names no EPSG code, so it cannot be put in EPSG:{target}",
                object.title
            )));
        };
        if source == target {
            return Ok(placed);
        }
        let Some(backend) = self.reprojector else {
            return Err(Error::Placement(format!(
                "`{}` is in EPSG:{source} and this export wants EPSG:{target}; \
                 supply a `Reprojector` — `energyml_proj::Proj4` is one",
                object.title
            )));
        };
        Ok(backend.reproject_all(&placed, source, target)?)
    }
}

/// Writes geometry in one interchange format.
///
/// The contract is the crate's: **what cannot be written is an error naming
/// it**. An exporter that quietly skips an object it does not understand
/// produces a file that looks complete and is not, and nothing downstream can
/// tell.
pub trait Exporter: Send + Sync {
    /// The name a caller asks for — `obj`, `geojson`. Lower case, no dot.
    fn name(&self) -> &'static str;

    /// The file extension, without the dot.
    fn extension(&self) -> &'static str;

    /// The MIME type, for a caller serving this over HTTP.
    fn mime(&self) -> &'static str;

    /// One line on what it is, for `energyml export --list`.
    fn describe(&self) -> &'static str;

    /// Which coordinates it wants, unless the caller overrides it.
    fn placement(&self) -> Placement {
        Placement::Metres
    }

    /// Whether it can write this object at all.
    ///
    /// Asked before anything is written, so a caller can skip or report rather
    /// than find a half-written file.
    fn handles(&self, object: &SceneObject) -> bool;

    /// [`Exporter::handles`], for the exporters whose answer depends on the
    /// options — GeoJSON accepts an object with no EPSG code once
    /// [`Options::native_crs`] is set.
    fn handles_with(&self, object: &SceneObject, options: &Options<'_>) -> bool {
        self.handles(object) && options.can_place_vertical(object)
    }

    /// Write one object.
    fn write(&self, object: &SceneObject, out: &mut dyn Write, options: &Options<'_>) -> Result<()>;

    /// Write several into one file.
    ///
    /// The default writes them in order, which is right for the formats where
    /// concatenation is the format — OBJ is one. A format with a single
    /// enclosing document, like GeoJSON, overrides this.
    fn write_all(&self, objects: &[SceneObject], out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
        for object in objects {
            self.write(object, out, options)?;
        }
        Ok(())
    }
}

/// The object an exporter should actually write: the same one, minus any node
/// that has no position.
///
/// **Every exporter calls this first, and the rule has one implementation.** A
/// node whose coordinates are NaN cannot be written in any of these formats —
/// OBJ has no syntax for it, VTK's readers put it at the origin or refuse the
/// file, and `NaN` is **not a JSON value**, so one null Z made a whole GeoJSON
/// unparseable by `JSON.parse`. Leaving each writer to remember that separately
/// is how two of the three would forget.
///
/// Borrowed when there is nothing to drop, which is the ordinary case: a package
/// that carries no null pays one pass over its points and no copy.
///
/// What the standard does and does not say about those nulls is set out in
/// [`energyml_geometry::defined`] — including the two RESQML flags that outrank
/// the coordinates for column-layer grids.
pub fn drawable(object: &SceneObject) -> std::borrow::Cow<'_, SceneObject> {
    match object.drawable_only() {
        Some(compacted) => std::borrow::Cow::Owned(compacted),
        None => std::borrow::Cow::Borrowed(object),
    }
}

/// The object an exporter should actually write: reshaped as the caller asked,
/// then stripped of the nodes that have no position.
///
/// **Every exporter calls this first, and the order of the two steps matters.**
/// Reshaping first is what makes a hole in a grid mean something: the lattice
/// becomes quadrilaterals, and the compaction then drops the four cells around
/// the null node instead of one dot among three hundred thousand. Compacting
/// first would leave the reshaping a rectangle with a gap in it, which is not a
/// rectangle.
///
/// A shape the object cannot take is an **error**, not a fallback: the caller
/// wrote a rule, and a rule that silently does something else teaches them it
/// worked. See [`crate::shapes`].
pub fn prepare<'a>(
    object: &'a SceneObject,
    options: &Options<'_>,
) -> Result<std::borrow::Cow<'a, SceneObject>> {
    let shape = options.shapes.shape_for(&object.qualified_type);
    let reshaped = object.as_shape(shape)?;
    Ok(match reshaped {
        std::borrow::Cow::Borrowed(same) => drawable(same),
        std::borrow::Cow::Owned(owned) => {
            std::borrow::Cow::Owned(owned.drawable_only().unwrap_or(owned))
        }
    })
}

/// The exporters a build offers.
pub struct Exporters {
    entries: Vec<Box<dyn Exporter>>,
}

impl Default for Exporters {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Exporters {
    /// None at all.
    pub fn none() -> Self {
        Self { entries: Vec::new() }
    }

    /// Everything this crate implements.
    pub fn builtin() -> Self {
        let mut exporters = Self::none();
        exporters.entries.push(Box::new(crate::obj::ObjExporter));
        exporters.entries.push(Box::new(crate::csv::CsvExporter));
        exporters.entries.push(Box::new(crate::geojson::GeoJsonExporter));
        exporters
            .entries
            .push(Box::new(crate::vtk::VtkExporter::legacy()));
        exporters
            .entries
            .push(Box::new(crate::vtk::VtkExporter::poly_data()));
        exporters
            .entries
            .push(Box::new(crate::vtk::VtkExporter::unstructured_grid()));
        exporters
    }

    /// Add one, found **before** everything already there, so a caller can
    /// replace a built-in as well as extend the set.
    pub fn push(&mut self, exporter: Box<dyn Exporter>) -> &mut Self {
        self.entries.insert(0, exporter);
        self
    }

    /// The exporter for a format name, matched case-insensitively and with a
    /// leading dot forgiven — `obj`, `OBJ` and `.obj` are one request.
    pub fn find(&self, name: &str) -> Option<&dyn Exporter> {
        let wanted = name.trim().trim_start_matches('.').to_ascii_lowercase();
        self.entries
            .iter()
            .find(|e| e.name() == wanted || e.extension() == wanted)
            .map(|e| e.as_ref())
    }

    /// The exporter for a format name, or an error listing what there is.
    pub fn get(&self, name: &str) -> Result<&dyn Exporter> {
        self.find(name).ok_or_else(|| Error::UnknownFormat {
            name: name.to_string(),
            available: self
                .entries
                .iter()
                .map(|e| e.name())
                .collect::<Vec<_>>()
                .join(", "),
        })
    }

    /// Every exporter, in the order they are searched.
    pub fn all(&self) -> Vec<&dyn Exporter> {
        self.entries.iter().map(|e| e.as_ref()).collect()
    }

    /// How many there are.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
