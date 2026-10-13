//! Points that live on lines: parametric geometry.
//!
//! Instead of storing a point, the file stores **which line it is on** and
//! **how far along**. The lines are given by control points; a parameter picks
//! a place between them. For a pillar that means "the same pillar, at depth
//! 300, then 500"; for a wellbore it means measured depth along the hole.
//!
//! That buys real compactness — a grid of a million cells shares its pillars —
//! and it costs an evaluation that has to know what kind of line each one is.
//!
//! # The kinds
//!
//! `LineKindIndices` names the interpolation, one per line. The table is the
//! schema's own, quoted from `ParametricLineArray`:
//!
//! | kind | what it is |
//! |---|---|
//! | −1 | null: no line |
//! | 0 | vertical |
//! | 1 | linear spline |
//! | 2 | natural cubic spline |
//! | 3 | cubic spline (tangential) |
//! | 4 | Z linear cubic spline |
//! | 5 | minimum-curvature spline |
//!
//! **This table was wrong here until 01/08.** It was written shifted by one —
//! −1 read as vertical, 0 as linear — which made the four splines refuse under
//! each other's names and, worse, made kind 1 (a *linear* spline, the simplest
//! thing in the list) refuse as "a natural cubic spline". The corpus settles
//! it: `testingPackageCpp`'s trajectory `acd2cdcf` declares kind 0 and holds a
//! line whose x and y never move — a vertical well, not a linear spline — and
//! grid `3ce91933` declares kinds 1, 4, 0, 0, 0, 0 for six pillars of which
//! four have a single control point, which only a *vertical* reading of 0
//! makes sense of.
//!
//! # How each is evaluated
//!
//! **Vertical.** *"Control points are (X,Y,-). Parameter values are
//! interpreted as depth => (X,Y,Z), **where the depth to Z conversion depends
//! on the vertical CRS direction**."* — RESQML TRG v2.2 §3.8, word for word,
//! and identical in the 2.0.1 guide. So x and y come from the control point,
//! and the parameter becomes the third coordinate **through the CRS**: a
//! depth-positive-down system stores the depth as it stands, an
//! elevation-positive-up one stores its negation. See [`Vertical`] for the
//! defect that clause exists to prevent, which was live here until 06/08.
//! A line left with a single control point
//! after padding is read this way whatever its declared kind — there is
//! nothing to interpolate between.
//!
//! **Linear.** Piecewise linear in the parameter.
//!
//! **Natural cubic.** The classic: second derivatives at the knots from the
//! tridiagonal system, vanishing at both ends. *"Interpolating basis functions
//! are obtained by specifying values and second derivatives at the knots."*
//! With two control points every second derivative is zero and it degenerates
//! to a straight segment, which is the schema's own special case (1).
//!
//! **Z linear cubic.** *"(X,Y) follow a natural cubic spline and Z follows a
//! linear spline."* Exactly that — the same spline as above for two of the
//! three components.
//!
//! **Tangential cubic.** Cubic Hermite: values and *first* derivatives at the
//! knots, the derivatives being `TangentVectors` when the file carries them.
//! When it does not, the schema says to compute them *"from the control point
//! data using a quadratic fit to the control point and the two adjacent
//! control points (if internal) or, if at an edge, by a vanishing second
//! derivative"* — which is what `derived_tangents` does below, and which is why
//! the fallback "reduces locally to a natural spline".
//!
//! **Minimum curvature.** *"Interpolating minimum-curvature basis functions
//! obtained by a circular arc construction. This differs from the 'drilling'
//! algorithm in which the parameter must be arc length."* Between two knots
//! the direction is rotated from one tangent to the other at a constant rate,
//! and the position is its integral — the arc of a circle. Where the data is
//! arc-consistent this reproduces the drilling formula exactly, dogleg angle
//! and ratio factor included; where it is not, the arc would miss the far
//! knot, so a linear term closes the gap. That closing term is **a choice, not
//! a quotation**: the standard says "interpolating" without saying what to do
//! when the arc cannot interpolate, and leaving the curve short of its own
//! knot is not an option.
//!
//! **Checked against the Technical Usage Guide on 05/09, and it stays a
//! choice.** §5.3.5 poses the problem the other way round: given a station, a
//! parameter step and two tangents, *"what is the position of the next
//! station"* — the far knot is an **output** there, so an arc that misses it
//! cannot arise and the guide has nothing to say about it. In a RESQML
//! parametric line every knot carries a stated control point, so the file can
//! and does state a position the arc does not reach. The gap is real, the
//! guide is silent, and the closing term remains this crate's.
//!
//! What the same section *does* settle is the **near-parallel** case, and the
//! check found a defect there: see `STRAIGHT` below.
//!
//! # Padding
//!
//! `KnotCount` is *"typically chosen to be the maximum number of control
//! points … on any parametric line"*, and shorter lines are *"pad\[ded\] with
//! NaN values"*. So a line's real length is its leading run of finite control
//! points, and everything downstream — the spline solve above all — sees only
//! that. Reading the padding as data puts NaN in the tridiagonal system and
//! poisons the whole line.
//!
//! # Off the end of a line
//!
//! A parameter outside the knots **extends the first or last interval**. The
//! line is unbounded, and both versions say so:
//!
//! > *"If isBounded=true in the line definition, then any out of range
//! > parametric values in the parametric points are truncated to the first or
//! > last control point. Otherwise, the interpolant in the first or last
//! > interval is used as an extrapolating function."* (2.0.1, `Geometry.xsd`,
//! > on `ParametricLineArray`)
//!
//! > *"In general, a parametric line is unbounded so the interpolant in the
//! > first or last interval is used as an extrapolating function."* (2.2
//! > technical reference, §3.8)
//!
//! `isBounded` exists in neither schema, so no file can ask for truncation:
//! the "otherwise" branch is the only readable one. This reader clamped until
//! 16/09/2026, which was reading a flag nobody can write — and it showed on a
//! real grid. In Volve's `Volve_SGrid` (100 × 186 × 50, 54 927 split
//! coordinate lines), **90 738 of 3 764 514 parameters lie outside their
//! line's control points, 72 384 of them on split lines**: exactly the fault
//! zones. Clamped, the cells on one side of a fault collapsed onto the
//! pillar's last control point — up to 130 m off — and the grid drew faces
//! bridging the throw. Read as the standard says, those cells land where the
//! HDF5 puts them (checked point by point against a reading in Python).

use energyml_core::value::{FieldValue, Reflect};

use crate::error::{Error, Result};
use crate::points::Point;
use crate::source::ValueSource;
use crate::values;

/// Below this, a length or a spacing is treated as zero.
const TINY: f64 = 1e-12;

/// Which way the local vertical axis counts, for the one place it decides a
/// coordinate rather than a placement.
///
/// # The bug this type exists to end
///
/// A **vertical** parametric line stores no z: the parameter *is* the depth,
/// and the standard says in one clause how to turn it into a coordinate —
/// *"the depth to Z conversion depends on the vertical CRS direction"*. This
/// module used to write `z = parameter` and stop there, which is right for a
/// `ZIncreasingDownward` system and **mirrors the line about the datum** in an
/// elevation system: a well drawn going up, its logs with it, while every
/// surface of the same package — whose points are stored, not computed — stayed
/// where it belonged. Reported on a model built from BRGM well data, where the
/// wells hung above the horizons they were made from.
///
/// It is not a placement, and that is why it lives in the reader rather than in
/// [`energyml_domain::crs::Crs::to_projected`]: the placement layer turns local
/// coordinates into projected ones and runs **once**, on everything. Doing the
/// depth-to-z conversion there as well would flip the line a second time. What
/// happens here is the earlier question — *what number does this file mean* —
/// and the reader is the only one that can answer it, because only the reader
/// knows the parameter was a depth.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vertical {
    /// The local z counts **downward**: a depth is stored as it stands.
    Down,
    /// The local z counts **upward**: a depth becomes its negation.
    Up,
}

impl Vertical {
    /// The coordinate a depth becomes in this system.
    fn z(self, depth: f64) -> f64 {
        match self {
            Vertical::Down => depth,
            Vertical::Up => -depth,
        }
    }

    /// What a CRS says, or the reading to take when nothing says anything.
    ///
    /// A geometry naming no CRS, or naming one this package does not hold, has
    /// stated no direction — and nothing downstream will place it either, since
    /// `SceneObject::place` leaves points alone when there is no CRS. Both ends
    /// then agree to leave the file's own numbers as they are, which is what
    /// `Down` means here: the parameter is a depth and stays one.
    pub fn of(crs: Option<&energyml_domain::crs::Crs>) -> Self {
        match crs.map(|crs| crs.vertical_direction) {
            Some(energyml_domain::crs::VerticalDirection::Up) => Vertical::Up,
            _ => Vertical::Down,
        }
    }

    /// The direction the object naming a CRS sits in.
    pub fn read(object: &dyn Reflect, source: &dyn ValueSource) -> Self {
        // A CRS that cannot be read is not a reason to refuse a geometry: the
        // reader's answer is then the file's own numbers, and `crs::of` has
        // already reported what it could not resolve.
        Self::of(energyml_domain::crs::of(object, source).ok().flatten().as_ref())
    }
}

/// The interpolation of one parametric line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// −1 — the line does not exist. Its points have no position.
    Null,
    /// 0 — x and y from the control point, the parameter is the third coordinate.
    Vertical,
    /// 1 — piecewise linear in the parameter.
    Linear,
    /// 2 — natural cubic spline.
    NaturalCubic,
    /// 3 — cubic Hermite through the tangents.
    TangentialCubic,
    /// 4 — x and y natural cubic, z piecewise linear.
    ZLinearCubic,
    /// 5 — circular-arc minimum curvature.
    MinimumCurvature,
}

impl Kind {
    /// The kind an index names, or `None` for one the standard does not define.
    fn of(index: i64) -> Option<Self> {
        Some(match index {
            -1 => Kind::Null,
            0 => Kind::Vertical,
            1 => Kind::Linear,
            2 => Kind::NaturalCubic,
            3 => Kind::TangentialCubic,
            4 => Kind::ZLinearCubic,
            5 => Kind::MinimumCurvature,
            _ => return None,
        })
    }

    /// Whether x and y follow a natural cubic spline.
    fn cubic(self) -> bool {
        matches!(self, Kind::NaturalCubic | Kind::ZLinearCubic)
    }

    /// Whether the evaluation needs a derivative at every knot.
    fn hermite(self) -> bool {
        matches!(self, Kind::TangentialCubic | Kind::MinimumCurvature)
    }
}

/// Evaluate a `Point3dParametricArray`.
///
/// The points are taken to be laid out over the array's **own** lines, one slot
/// per line per level. That holds whenever a thing has as many coordinate lines
/// as parametric ones — which is every case but a faulted grid. For that one,
/// see [`read_on_lines`].
pub fn read(
    array: &dyn Reflect,
    source: &dyn ValueSource,
    at: &str,
    vertical: Vertical,
) -> Result<Vec<Point>> {
    read_on_lines(array, source, at, None, vertical)
}

/// Evaluate a `Point3dParametricArray` whose slots do **not** stand one-to-one
/// against its parametric lines.
///
/// `on[c]` is the parametric line that coordinate line `c` evaluates against,
/// and the points are laid out level-major over `on.len()` coordinate lines.
///
/// This exists for one case, and it is not exotic — it is every faulted grid.
/// A fault displaces the rock on one side of a pillar relative to the other, so
/// RESQML gives that pillar a **split coordinate line**: a second column of
/// points for the columns on the far side. The split is *the same geometric
/// line* — same control points, same kind — carrying different parameters. A
/// grid with 6 pillars and 2 splits therefore has **8 coordinate lines and 6
/// parametric lines**, and its `Parameters` array is shaped over the 8.
///
/// Read as though the two counts agreed, everything after the first level lands
/// on the wrong line: the parameter meant for pillar 0 at level 1 is evaluated
/// against pillar 2, and so on around a cycle of six. The depths stay plausible
/// because they *are* real depths — only the x and y come from the wrong
/// pillar. What comes out is a mesh of the right size, in the right place, with
/// its faces pointing in every direction. A picture of nothing that looks like
/// a picture of something.
///
/// Nothing caught it, and that is the part worth keeping: [`crate::ijk`]
/// checks the point count against `levels × coordinate lines` and was
/// satisfied, because the total *was* right. The two readers disagreed about
/// what a point index meant while agreeing on how many there were.
pub fn read_on_lines(
    array: &dyn Reflect,
    source: &dyn ValueSource,
    at: &str,
    on: Option<&[usize]>,
    // `vertical`: which way the local z counts, from the **geometry** that
    // holds this array. The array itself names no CRS, so it cannot ask.
    vertical: Vertical,
) -> Result<Vec<Point>> {
    let Some(parameters) = field_object(array, "Parameters") else {
        return Err(Error::malformed(at, "a parametric array with no `Parameters`"));
    };
    let parameters = values::read_doubles(parameters, source, &format!("{at}.Parameters"))?;

    let Some(lines) = field_object(array, "ParametricLines") else {
        return Err(Error::malformed(
            at,
            "a parametric array with no `ParametricLines`",
        ));
    };
    let lines = Lines::read(lines, source, at, vertical)?;

    // Which line each point sits on. Three answers, most specific first: the
    // file says (`ParametricLineIndices`); the caller says (`on`, which is how
    // a faulted grid states that its coordinate lines outnumber its parametric
    // ones); or nobody says, and the points are laid out line-fastest in pillar
    // order, so the line is the position modulo the line count.
    let explicit = match field_object(array, "ParametricLineIndices") {
        Some(indices) => Some(values::read_integers(
            indices,
            source,
            &format!("{at}.ParametricLineIndices"),
        )?),
        None => None,
    };
    if let Some(indices) = &explicit
        && indices.len() != parameters.len()
    {
        return Err(Error::malformed(
            at,
            format!(
                "{} parameter(s) against {} line index/indices",
                parameters.len(),
                indices.len()
            ),
        ));
    }

    // A caller that states the layout states it completely: the number of
    // parameters has to be a whole number of levels over the coordinate lines
    // it named, or the two disagree about the shape and going on would repeat
    // the very bug this argument exists to fix.
    if let Some(on) = on {
        if on.is_empty() {
            return Err(Error::malformed(at, "a parametric array on no coordinate lines"));
        }
        if parameters.len() % on.len() != 0 {
            return Err(Error::malformed(
                at,
                format!(
                    "{} parameter(s) over {} coordinate line(s) is not a whole number of levels",
                    parameters.len(),
                    on.len()
                ),
            ));
        }
    }

    let count = lines.lines.len().max(1);
    let mut points = Vec::with_capacity(parameters.len());
    for (index, parameter) in parameters.iter().enumerate() {
        let line = match (&explicit, on) {
            (Some(indices), _) => indices[index].max(0) as usize,
            (None, Some(on)) => on[index % on.len()],
            (None, None) => index % count,
        };
        let Some(line) = lines.lines.get(line) else {
            return Err(Error::malformed(
                at,
                format!("line {line} of {} lines", lines.lines.len()),
            ));
        };
        points.push(line.at(*parameter));
    }
    Ok(points)
}

/// The lines a parametric array evaluates against.
struct Lines {
    lines: Vec<Line>,
}

impl Lines {
    /// Lines the array does not carry: a regular selection of **another
    /// representation's** lines.
    ///
    /// The exact counterpart of [`crate::derived`], one level up: that one borrows
    /// a support's nodes, this one borrows its lines. The standard writes the two
    /// in the same sentences, and this module reads the selection through
    /// [`crate::derived::indices_on`] so that it cannot read them differently.
    ///
    /// # What the standard says, and it says it plainly
    ///
    /// The reference guide's entry for `ParametricLineFromRepresentationLatticeArray`
    /// is unusually complete, and identical word for word in 2.0.1 and 2.2:
    ///
    /// > BUSINESS RULE: The dimensions of the integer lattice array must be
    /// > consistent with the dimensions of the supporting representation.
    /// >
    /// > For a column-layer grid, the parametric lines follow the indexing of the
    /// > pillars.
    /// >
    /// > BUSINESS RULE: The start value of the integer lattice array must be the
    /// > linearized index of the starting line. Example: `iStart + ni * jStart` in
    /// > case of a supporting 2D grid.
    ///
    /// Three things, and each answers a question that had to be guessed for the
    /// node version: the lattice has **one axis per axis of the support**; the
    /// start is **flat**, with the flattening spelled out; and for a grid the lines
    /// **are** the pillars, in pillar order — `(Ni + 1) × (Nj + 1)`, i fastest.
    ///
    /// Those rules were in this workspace's own generated code the whole time. The
    /// XSD carries them as annotations and the generator copies annotations, so
    /// they sit in the doc comment on
    /// `ParametricLineFromRepresentationLatticeArray::line_indices_on_supporting_representation`.
    ///
    /// # No file in the corpus has one
    ///
    /// Which is why this is written against the standard and tested against a
    /// borrowing built in the test: a grid's own lines, selected whole, must
    /// evaluate to exactly what the grid's own array evaluates to. See
    /// `a_borrowed_line_lattice_is_the_supports_own_lines`.
    fn borrowed(
        object: &dyn Reflect,
        source: &dyn ValueSource,
        at: &str,
        vertical: Vertical,
    ) -> Result<Self> {
        let Some(support_uri) = crate::well::reference_of(object, "SupportingRepresentation") else {
            return Err(Error::malformed(
                at,
                "a line lattice over a representation that it does not name",
            ));
        };
        let Some(support) = source.object_at(&support_uri) else {
            return Err(Error::Domain(energyml_domain::Error::Missing(format!(
                "the supporting representation {support_uri}"
            ))));
        };
        let support = support.as_reflect();

        // The support's own lines. A representation that has none has nothing
        // to lend, and saying which one is the whole message.
        let Some(array) = find_line_array(support) else {
            return Err(Error::malformed(
                at,
                format!("the support {support_uri} carries no `ParametricLineArray` to borrow from"),
            ));
        };
        // The borrower's own sense, and it is the support's: a representation
        // that borrows another's lines is in that other's system — the same
        // rule `crs::of` follows to give the two the same CRS.
        let lines = Self::read(array, source, &format!("{at}.{support_uri}"), vertical)?;

        // *"For a column-layer grid, the parametric lines follow the indexing
        // of the pillars"* — so the shape is the pillar lattice, slowest axis
        // first, and a support that is not a grid is one flat list of lines.
        let shape = pillar_shape(support).unwrap_or_else(|| vec![lines.lines.len()]);
        let stated: usize = shape.iter().product();
        if stated != lines.lines.len() {
            return Err(Error::malformed(
                at,
                format!(
                    "the support {support_uri} states a {shape:?} pillar lattice — {stated} line(s) — \
                     and carries {}",
                    lines.lines.len()
                ),
            ));
        }

        let Some(lattice) = field_object(object, "LineIndicesOnSupportingRepresentation") else {
            return Err(Error::malformed(
                at,
                "a line lattice with no `LineIndicesOnSupportingRepresentation`",
            ));
        };
        let chosen = crate::derived::indices_on(lattice, &shape, at)?;
        Ok(Self {
            lines: chosen
                .into_iter()
                .map(|index| lines.lines[index].clone())
                .collect(),
        })
    }

    fn read(object: &dyn Reflect, source: &dyn ValueSource, at: &str, vertical: Vertical) -> Result<Self> {
        let name = object.type_info().name;
        if name == "ParametricLineFromRepresentationLatticeArray" {
            return Self::borrowed(object, source, at, vertical);
        }
        if !name.ends_with("ParametricLineArray") {
            return Err(Error::unsupported(leak(name), at));
        }
        let knots = values::scalar_i64(object, "KnotCount").unwrap_or(0).max(0) as usize;
        if knots == 0 {
            return Err(Error::malformed(at, "`ParametricLineArray` with no `KnotCount`"));
        }
        let Some(control) = field_object(object, "ControlPoints") else {
            return Err(Error::malformed(
                at,
                "`ParametricLineArray` with no `ControlPoints`",
            ));
        };
        // Shaped `[knots][lines][3]`: knot slowest, so one line's control
        // points are `lines` apart rather than adjacent.
        let flat = crate::points::read(control, source, &format!("{at}.ControlPoints"), vertical)?;
        if flat.len() % knots != 0 {
            return Err(Error::malformed(
                at,
                format!(
                    "{} control point(s) is not a multiple of {knots} knot(s)",
                    flat.len()
                ),
            ));
        }
        let count = flat.len() / knots;
        let control = per_line(&flat, count);

        let parameters = match field_object(object, "ControlPointParameters") {
            Some(array) => {
                let flat = values::read_doubles(array, source, &format!("{at}.ControlPointParameters"))?;
                if flat.len() != count * knots {
                    return Err(Error::malformed(
                        at,
                        format!(
                            "{} control point parameter(s) for {count} line(s) of {knots} knot(s)",
                            flat.len()
                        ),
                    ));
                }
                Some(per_line(&flat, count))
            }
            None => None,
        };

        let tangents = match field_object(object, "TangentVectors") {
            Some(array) => {
                let flat = crate::points::read(array, source, &format!("{at}.TangentVectors"), vertical)?;
                if flat.len() != count * knots {
                    return Err(Error::malformed(
                        at,
                        format!(
                            "{} tangent vector(s) for {count} line(s) of {knots} knot(s)",
                            flat.len()
                        ),
                    ));
                }
                Some(per_line(&flat, count))
            }
            None => None,
        };

        let kinds = match field_object(object, "LineKindIndices") {
            Some(array) => values::read_integers(array, source, &format!("{at}.LineKindIndices"))?,
            // No kinds stated. One knot can only be a vertical line; more than
            // one is read as the linear spline, kind 1.
            None => vec![if knots == 1 { 0 } else { 1 }; count],
        };

        let mut lines = Vec::with_capacity(count);
        for index in 0..count {
            let declared = kinds.get(index).copied().unwrap_or(0);
            let Some(kind) = Kind::of(declared) else {
                // Refused rather than guessed: an index the standard does not
                // define could be anything, and every remaining kind curves
                // differently.
                return Err(Error::unsupported(
                    "an unknown kind of parametric line",
                    format!("{at} line {index}, kind {declared}"),
                ));
            };
            lines.push(Line::new(
                kind,
                control[index].clone(),
                parameters.as_ref().map(|p| p[index].clone()),
                tangents.as_ref().map(|t| t[index].clone()),
                vertical,
            ));
        }
        Ok(Self { lines })
    }
}

/// Split a `[knot][line]` array into one vector per line, knot order kept.
fn per_line<T: Copy>(flat: &[T], count: usize) -> Vec<Vec<T>> {
    let count = count.max(1);
    let mut out = vec![Vec::with_capacity(flat.len() / count); count];
    for (index, value) in flat.iter().enumerate() {
        out[index % count].push(*value);
    }
    out
}

/// One parametric line, trimmed of its padding and ready to evaluate.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Line {
    kind: Kind,
    control: Vec<Point>,
    /// The parameter of each control point, ascending.
    knots: Vec<f64>,
    /// Second derivatives at the knots — empty unless the kind is cubic.
    moments: Vec<Point>,
    /// dP/dparameter at the knots — empty unless the kind is Hermite.
    tangents: Vec<Point>,
    /// Whether the knots were read from the file rather than assumed 0, 1, 2 …
    ///
    /// It matters in one place, and there it matters a lot: see [`Line::stations`].
    parametrised: bool,
    /// Which way the local z counts — the clause the standard attaches to the
    /// vertical kind, and nothing else. See [`Vertical`].
    vertical: Vertical,
}

impl Line {
    fn new(
        kind: Kind,
        control: Vec<Point>,
        knots: Option<Vec<f64>>,
        tangents: Option<Vec<Point>>,
        vertical: Vertical,
    ) -> Self {
        // The real length of the line: its leading run of finite control
        // points, and of finite parameters when it has them.
        let mut length = control
            .iter()
            .position(|p| p.iter().any(|c| !c.is_finite()))
            .unwrap_or(control.len());
        if let Some(knots) = &knots {
            length = length.min(knots.iter().position(|k| !k.is_finite()).unwrap_or(knots.len()));
        }

        let parametrised = knots.is_some();
        let mut control: Vec<Point> = control[..length].to_vec();
        // No parameters given: the knots are at 0, 1, 2 … — what the schema
        // says to assume, and what a caller has no other way to know.
        let mut knots: Vec<f64> = match knots {
            Some(knots) => knots[..length].to_vec(),
            None => (0..length).map(|index| index as f64).collect(),
        };
        let mut tangents: Vec<Point> = match tangents {
            Some(tangents) => tangents[..length.min(tangents.len())].to_vec(),
            None => Vec::new(),
        };

        // The schema requires the parameters to increase along the line, and
        // files write them decreasing when the CRS counts depth downward. The
        // curve is the same either way, so the line is stored ascending and
        // every evaluation below can assume it. Nothing is negated: the
        // parameter axis is untouched, only the order the knots are visited.
        if knots.len() > 1 && knots[knots.len() - 1] < knots[0] {
            control.reverse();
            knots.reverse();
            tangents.reverse();
        }

        let moments = if kind.cubic() && control.len() > 2 {
            moments(&knots, &control)
        } else {
            Vec::new()
        };
        let tangents = if kind.hermite() && control.len() > 1 {
            if tangents.len() == control.len() {
                tangents
            } else {
                // "If a tangent vector is missing, then it is computed in the
                // same fashion as for a natural cubic spline." A partial array
                // is treated as absent: the schema promises one vector per
                // control point for these kinds, and mixing given and derived
                // ones would put a seam in the curve where the array ran out.
                derived_tangents(&knots, &control)
            }
        } else {
            Vec::new()
        };

        Self {
            kind,
            control,
            knots,
            moments,
            tangents,
            parametrised,
            vertical,
        }
    }

    /// The line at its own knots: the survey stations, in order.
    ///
    /// A spline passes through its knots, so for the curved kinds this is the
    /// control points and nothing more. It differs for a **vertical** line,
    /// where the schema says the control points are `(X,Y,-)` and the third
    /// coordinate comes from the parameter — so the stations are the ones the
    /// parameters put there, not whatever was written in the unused slot.
    ///
    /// Unless the file gave no parameters at all. Then the knots here are the
    /// assumed 0, 1, 2 … and feeding them back would stack a well's stations
    /// one metre apart; the control points are the only thing left that means
    /// something, so they are what comes out.
    pub(crate) fn stations(&self) -> Vec<Point> {
        if !self.parametrised {
            return self.control.clone();
        }
        self.knots.iter().map(|knot| self.at(*knot)).collect()
    }

    /// One line borrowed from another representation.
    ///
    /// `ParametricLineFromRepresentationGeometry` is the singular of
    /// [`Lines::borrowed`], and it is written as the same three steps on
    /// purpose: name the support, find the array it lends, take what was asked
    /// for. A second reading of *which representation, and which of its lines*
    /// would be a second chance to disagree with the plural one.
    ///
    /// **The index field is spelled two ways, and one of them is a typo in the
    /// schema itself**: RESQML 2.0.1 writes `LineIndiexOnSupportingRepresentation`
    /// (sic), 2.2 corrected it to `LineIndexOnSupportingRepresentation`. Reading
    /// only the corrected spelling refuses every 2.0.1 file and reading only the
    /// typo refuses every 2.2 one, so both are read — as everywhere a release
    /// respells a field.
    ///
    /// No file in the corpus has one, so this is written against the standard
    /// and tested against a borrowing built in the test: a support's own line,
    /// borrowed, must evaluate to what the support's own array gives.
    fn borrowed_one(object: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Self> {
        // The borrower's own sense, and it is the support's — the same rule the
        // plural form follows.
        let vertical = Vertical::read(object, source);
        let Some(support_uri) = crate::well::reference_of(object, "SupportingRepresentation") else {
            return Err(Error::malformed(
                at,
                "a borrowed line over a representation that it does not name",
            ));
        };
        let Some(support) = source.object_at(&support_uri) else {
            return Err(Error::Domain(energyml_domain::Error::Missing(format!(
                "the supporting representation {support_uri}"
            ))));
        };
        let support = support.as_reflect();
        let Some(array) = find_line_array(support) else {
            return Err(Error::malformed(
                at,
                format!("the support {support_uri} carries no `ParametricLineArray` to borrow from"),
            ));
        };
        let lines = Lines::read(array, source, &format!("{at}.{support_uri}"), vertical)?;

        let Some(index) = LINE_INDEX
            .iter()
            .find_map(|field| values::scalar_i64(object, field))
        else {
            return Err(Error::malformed(
                at,
                "a borrowed line that does not say which line of the support it is",
            ));
        };
        // Out of range names no line, and the message carries the count: a
        // borrower asking for line 12 of a six-line support has said something
        // about the support, and folding the index back into range would hide
        // it.
        if index < 0 || index as usize >= lines.lines.len() {
            return Err(Error::malformed(
                at,
                format!(
                    "line {index} of the support {support_uri}, which lends {}",
                    lines.lines.len()
                ),
            ));
        }
        Ok(lines.lines[index as usize].clone())
    }

    /// A single `ParametricLineGeometry` — one line, its knots, its kind.
    pub(crate) fn single(geometry: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Self> {
        let name = geometry.type_info().name;
        if name == "ParametricLineFromRepresentationGeometry" {
            return Self::borrowed_one(geometry, source, at);
        }
        if !name.ends_with("ParametricLineGeometry") {
            return Err(Error::unsupported(leak(name), at));
        }
        // Resolved before anything is read: the geometry names its own
        // `LocalCrs`, so no caller has to remember to pass it and none can pass
        // the wrong one.
        let vertical = Vertical::read(geometry, source);
        let Some(control) = field_object(geometry, "ControlPoints") else {
            return Err(Error::malformed(at, "a parametric line with no `ControlPoints`"));
        };
        let control = crate::points::read(control, source, &format!("{at}.ControlPoints"), vertical)?;
        if control.is_empty() {
            return Err(Error::malformed(at, "a parametric line with no control point"));
        }

        let parameters = match field_object(geometry, "ControlPointParameters") {
            Some(array) => {
                let read = values::read_doubles(array, source, &format!("{at}.ControlPointParameters"))?;
                if read.len() != control.len() {
                    // "BUSINESS RULE: The size MUST match the number of control
                    // points." A file that breaks it has said two different
                    // things about how long the line is.
                    return Err(Error::malformed(
                        at,
                        format!(
                            "{} control point parameter(s) for {} control point(s)",
                            read.len(),
                            control.len()
                        ),
                    ));
                }
                Some(read)
            }
            None => None,
        };
        let tangents = match field_object(geometry, "TangentVectors") {
            Some(array) => Some(crate::points::read(
                array,
                source,
                &format!("{at}.TangentVectors"),
                vertical,
            )?),
            None => None,
        };

        let declared = values::scalar_i64(geometry, "LineKindIndex").unwrap_or(0);
        let Some(kind) = Kind::of(declared) else {
            return Err(Error::unsupported(
                "an unknown kind of parametric line",
                format!("{at}, kind {declared}"),
            ));
        };
        Ok(Self::new(kind, control, parameters, tangents, vertical))
    }

    /// The point at `parameter` along the line.
    pub(crate) fn at(&self, parameter: f64) -> Point {
        /// A point the file declares to have no position.
        const NOWHERE: Point = [f64::NAN; 3];

        if self.kind == Kind::Null || self.control.is_empty() {
            // "null value: no line" — and a point on a line that does not
            // exist has no coordinates. NaN is how RESQML itself writes an
            // absent point, and the callers already count them out loud.
            return NOWHERE;
        }
        // A single control point determines nothing to interpolate along, so
        // the parameter is the third coordinate — the vertical pillar case,
        // and the majority of the corpus's parametric grids.
        if self.kind == Kind::Vertical || self.control.len() == 1 {
            // *"Parameter values are interpreted as depth => (X,Y,Z), where the
            // depth to Z conversion depends on the vertical CRS direction."*
            // The conversion is the whole of this line's geometry: without it
            // the line is mirrored about the origin in an elevation system.
            let origin = self.control[0];
            return [origin[0], origin[1], self.vertical.z(parameter)];
        }

        // Unbounded: `segment` already answers with the first or the last
        // interval for a parameter off either end, and every interpolant here
        // takes a fraction outside 0…1 as the extrapolation the standard asks
        // for. Truncating instead is `isBounded=true`, which no schema has.
        let t = parameter;

        match self.kind {
            Kind::Linear => self.linear(t),
            Kind::NaturalCubic => self.cubic(t),
            Kind::ZLinearCubic => {
                let curved = self.cubic(t);
                let straight = self.linear(t);
                [curved[0], curved[1], straight[2]]
            }
            Kind::TangentialCubic => self.hermite(t),
            Kind::MinimumCurvature => self.arc(t),
            // Handled above; listed rather than caught by `_` so a new kind
            // cannot join the enumeration and silently land here.
            Kind::Null | Kind::Vertical => NOWHERE,
        }
    }

    /// The interval `t` falls in, as the index of its upper knot.
    fn segment(&self, t: f64) -> usize {
        let mut index = 1;
        while index + 1 < self.knots.len() && t > self.knots[index] {
            index += 1;
        }
        index
    }

    /// Where `t` sits in its interval: the interval, and 0…1 across it.
    fn place(&self, t: f64) -> (usize, f64) {
        let index = self.segment(t);
        let span = self.knots[index] - self.knots[index - 1];
        let fraction = if span.abs() < TINY {
            0.0
        } else {
            (t - self.knots[index - 1]) / span
        };
        (index, fraction)
    }

    fn linear(&self, t: f64) -> Point {
        let (index, fraction) = self.place(t);
        lerp(self.control[index - 1], self.control[index], fraction)
    }

    /// The natural cubic spline, from the second derivatives at the knots.
    fn cubic(&self, t: f64) -> Point {
        if self.moments.len() != self.control.len() {
            // Two control points, or a knot spacing the solve refused: every
            // second derivative is zero and the spline *is* the straight
            // segment. The schema says so itself for two points.
            return self.linear(t);
        }
        let index = self.segment(t);
        let span = self.knots[index] - self.knots[index - 1];
        if span.abs() < TINY {
            return self.control[index];
        }
        let a = (self.knots[index] - t) / span;
        let b = (t - self.knots[index - 1]) / span;
        let (p0, p1) = (self.control[index - 1], self.control[index]);
        let (m0, m1) = (self.moments[index - 1], self.moments[index]);
        let mut out = [0.0; 3];
        for axis in 0..3 {
            out[axis] = a * p0[axis]
                + b * p1[axis]
                + ((a * a * a - a) * m0[axis] + (b * b * b - b) * m1[axis]) * span * span / 6.0;
        }
        out
    }

    /// Cubic Hermite: values and first derivatives at the knots.
    fn hermite(&self, t: f64) -> Point {
        if self.tangents.len() != self.control.len() {
            return self.linear(t);
        }
        let index = self.segment(t);
        let span = self.knots[index] - self.knots[index - 1];
        if span.abs() < TINY {
            return self.control[index];
        }
        let s = (t - self.knots[index - 1]) / span;
        let (s2, s3) = (s * s, s * s * s);
        let (h00, h10) = (2.0 * s3 - 3.0 * s2 + 1.0, s3 - 2.0 * s2 + s);
        let (h01, h11) = (-2.0 * s3 + 3.0 * s2, s3 - s2);
        let (p0, p1) = (self.control[index - 1], self.control[index]);
        let (m0, m1) = (self.tangents[index - 1], self.tangents[index]);
        let mut out = [0.0; 3];
        for axis in 0..3 {
            out[axis] = h00 * p0[axis] + h01 * p1[axis] + (h10 * m0[axis] + h11 * m1[axis]) * span;
        }
        out
    }

    /// Minimum curvature: the arc of the circle the two tangents describe.
    ///
    /// The direction turns from one tangent to the other at a constant rate —
    /// `d(f) = (sin((1−f)α)·t̂₀ + sin(fα)·t̂₁) / sin α`, the shortest rotation
    /// between them — and the position is its integral, which is a circular
    /// arc. Its closed form is the `integral` below.
    ///
    /// Two things follow, and both are worth knowing:
    ///
    /// * On data that really lies on an arc, this **is** the drilling formula.
    ///   `∫₀¹ d` comes out parallel to `t̂₀ + t̂₁` with length `2·sin(α/2)/α`,
    ///   so the chord is `(L/2)·RF·(t̂₀+t̂₁)` with the ratio factor
    ///   `RF = (2/α)·tan(α/2)` — the expression every directional-drilling
    ///   package uses, arrived at rather than copied.
    /// * On data that does not, the arc ends somewhere other than the far
    ///   knot. The closing term puts it back, linearly across the interval, so
    ///   the curve still interpolates. That term is this crate's decision; the
    ///   standard says "interpolating" and stops.
    fn arc(&self, t: f64) -> Point {
        if self.tangents.len() != self.control.len() {
            return self.linear(t);
        }
        let (index, fraction) = self.place(t);
        let (p0, p1) = (self.control[index - 1], self.control[index]);
        let (Some(t0), Some(t1)) = (unit(self.tangents[index - 1]), unit(self.tangents[index])) else {
            // A zero tangent gives no direction to turn from.
            return lerp(p0, p1, fraction);
        };

        let alpha = dot(t0, t1).clamp(-1.0, 1.0).acos();
        // No dogleg is a straight segment; a dogleg of 180° has no arc through
        // it at all — the circle would be a point. Both are the straight
        // reading, and the second is the one that would divide by zero.
        if alpha < STRAIGHT || (std::f64::consts::PI - alpha) < 1e-9 {
            return lerp(p0, p1, fraction);
        }

        let scale = 1.0 / (alpha * alpha.sin());
        let cos = alpha.cos();
        let integral = |f: f64| -> Point {
            add(
                mul(t0, scale * ((1.0 - f) * alpha).cos() - scale * cos),
                mul(t1, scale * (1.0 - (f * alpha).cos())),
            )
        };

        let whole = integral(1.0);
        let length = norm(whole);
        if length < TINY {
            return lerp(p0, p1, fraction);
        }
        // The arc length that makes the chord come out the right size.
        let arc_length = norm(sub(p1, p0)) / length;
        let closing = sub(sub(p1, p0), mul(whole, arc_length));
        add(
            add(p0, mul(integral(fraction), arc_length)),
            mul(closing, fraction),
        )
    }
}

/// **One parametric line, evaluable anywhere along it** — what a caller needs
/// to place a parameter *between* two knots: a measured depth on a wellbore
/// trajectory, where a computed top falls (a geological
/// operation).
///
/// The reader of every parametric geometry of this crate, made public rather
/// than copied: a second evaluator of the same line would be a second answer
/// to *where is the hole at this depth*, and the minimum-curvature arc is
/// exactly what would come out different. [`crate::well::path_of`] is the way
/// to it from a trajectory.
#[derive(Clone, Debug, PartialEq)]
pub struct Path(Line);

impl Path {
    /// The line a `ParametricLineGeometry` states — kind, control points,
    /// parameters and tangents as the file writes them, the sense of its
    /// vertical read from its own `LocalCrs`.
    pub fn of(geometry: &dyn Reflect, source: &dyn ValueSource) -> Result<Path> {
        Line::single(geometry, source, "ParametricLineGeometry").map(Path)
    }

    /// A line from values: `kind` is the schema's `LineKindIndex` — an index
    /// the standard does not define is refused, like in a file — and one
    /// parameter per control point.
    pub fn new(
        kind: i64,
        control: Vec<Point>,
        parameters: Vec<f64>,
        tangents: Option<Vec<Point>>,
        vertical: Vertical,
    ) -> Result<Path> {
        let at = "a parametric line given as values";
        let Some(kind) = Kind::of(kind) else {
            return Err(Error::unsupported(
                "an unknown kind of parametric line",
                format!("{at}, kind {kind}"),
            ));
        };
        if control.is_empty() {
            return Err(Error::malformed(at, "no control point"));
        }
        if parameters.len() != control.len() {
            return Err(Error::malformed(
                at,
                format!(
                    "{} parameter(s) for {} control point(s)",
                    parameters.len(),
                    control.len()
                ),
            ));
        }
        if let Some(tangents) = &tangents
            && tangents.len() != control.len()
        {
            return Err(Error::malformed(
                at,
                format!(
                    "{} tangent(s) for {} control point(s)",
                    tangents.len(),
                    control.len()
                ),
            ));
        }
        Ok(Path(Line::new(
            kind,
            control,
            Some(parameters),
            tangents,
            vertical,
        )))
    }

    /// The point at `parameter` — past either end, the first or last interval
    /// extended, as the standard reads an unbounded line.
    pub fn at(&self, parameter: f64) -> Point {
        self.0.at(parameter)
    }

    /// The parameters of its knots, ascending — `None` when the file stated
    /// none: the knots are then the assumed 0, 1, 2 … and a parameter along
    /// the line is no measured depth.
    pub fn knots(&self) -> Option<&[f64]> {
        self.0.parametrised.then_some(self.0.knots.as_slice())
    }
}

/// Below this dogleg, the arc is read as a straight segment.
///
/// # Why it is not the smallest number that avoids a division by zero
///
/// It was `1e-9`, which is that number, and the Technical Usage Guide warns
/// against exactly this (§5.3.5): *"If the two tangent vectors are close to
/// parallel then α will be close to 0, and some care has to be taken when
/// evaluating these expressions."* The closed form divides by `α·sin α` and
/// subtracts two cosines that are both within `α²` of 1 — catastrophic
/// cancellation, and the guard let it happen over four decades.
///
/// Measured on the expression as written, with two unit tangents `α` apart:
///
/// | `α` | error of the closed form | error of the straight reading |
/// |---|---|---|
/// | 1e-8 | **the norm comes out 0** | 0.1 µm over 100 m |
/// | 1e-7 | 4 cm over 100 m | 1.3 µm |
/// | 1e-6 | 4.4 mm over 100 m | 12 µm |
/// | 1e-5 | 4 µm over 100 m | 125 µm |
///
/// The two cross between `1e-6` and `1e-5`: below that the closed form is
/// rounding noise, and above it the straight reading starts losing a real
/// curve. `1e-6` is the conservative side of the crossing — it keeps the arc
/// wherever the arc is worth having.
///
/// The straight reading **is** the limit the guide gives there: it prints the
/// series for `(1−cos α)/sin α → α/2` and adds that *"additional higher order
/// terms in α may also be included to improve upon this approximation"*. The
/// leading term is the straight segment, and the next is `O(α²)` relative —
/// under 1e-12 at this threshold, which is finer than the closed form manages
/// anywhere near it.
const STRAIGHT: f64 = 1e-6;

/// Second derivatives at the knots of the natural cubic spline through
/// `control`, with a vanishing second derivative at both ends.
///
/// The tridiagonal system `hᵢ·Mᵢ₋₁ + 2(hᵢ+hᵢ₊₁)·Mᵢ + hᵢ₊₁·Mᵢ₊₁ = 6·(Δᵢ₊₁ − Δᵢ)`,
/// solved once for all three components — the matrix depends only on the
/// knots, so x, y and z share the elimination.
fn moments(knots: &[f64], control: &[Point]) -> Vec<Point> {
    let n = control.len();
    let mut solved = vec![[0.0f64; 3]; n];
    if n < 3 || knots.len() != n {
        return solved;
    }
    let spacing: Vec<f64> = (1..n).map(|index| knots[index] - knots[index - 1]).collect();
    if spacing.iter().any(|h| h.abs() < TINY) {
        // Two knots at the same parameter: the schema forbids it ("strictly
        // monotonically increasing"), and the system is singular. Returning
        // zeros makes the line read as straight, which `cubic` says out loud
        // by falling back to `linear`.
        return Vec::new();
    }

    let mut super_diagonal = vec![0.0f64; n];
    let mut right = vec![[0.0f64; 3]; n];
    for index in 1..n - 1 {
        let (left, right_span) = (spacing[index - 1], spacing[index]);
        let denominator = 2.0 * (left + right_span) - left * super_diagonal[index - 1];
        super_diagonal[index] = right_span / denominator;
        for axis in 0..3 {
            let slope_after = (control[index + 1][axis] - control[index][axis]) / right_span;
            let slope_before = (control[index][axis] - control[index - 1][axis]) / left;
            right[index][axis] =
                (6.0 * (slope_after - slope_before) - left * right[index - 1][axis]) / denominator;
        }
    }
    for index in (1..n - 1).rev() {
        for axis in 0..3 {
            solved[index][axis] = right[index][axis] - super_diagonal[index] * solved[index + 1][axis];
        }
    }
    solved
}

/// The tangents the schema says to compute when the file gives none.
///
/// *"To obtain the tangent at internal knots, the control points are fit by a
/// quadratic function with the two adjacent control points. At edge knots, the
/// second derivative vanishes."* The first is the derivative of the parabola
/// through three points; the second, written out for a cubic Hermite piece,
/// is `m₀ = (3·Δ/h − m₁)/2` — the natural-spline end condition, which is why
/// the schema says the fallback "reduces locally to a natural spline".
fn derived_tangents(knots: &[f64], control: &[Point]) -> Vec<Point> {
    let n = control.len();
    let mut out = vec![[0.0f64; 3]; n];
    if n < 2 || knots.len() != n {
        return out;
    }
    let slope = |from: usize, to: usize| -> Option<Point> {
        let span = knots[to] - knots[from];
        (span.abs() >= TINY).then(|| mul(sub(control[to], control[from]), 1.0 / span))
    };
    if n == 2 {
        // Two knots, both with a vanishing second derivative: the only cubic
        // that satisfies both is the straight segment.
        let Some(slope) = slope(0, 1) else {
            return out;
        };
        return vec![slope, slope];
    }

    for index in 1..n - 1 {
        let before = knots[index] - knots[index - 1];
        let after = knots[index + 1] - knots[index];
        if before.abs() < TINY || after.abs() < TINY {
            continue;
        }
        for axis in 0..3 {
            out[index][axis] = -after / (before * (before + after)) * control[index - 1][axis]
                + (after - before) / (before * after) * control[index][axis]
                + before / (after * (before + after)) * control[index + 1][axis];
        }
    }
    if let Some(slope) = slope(0, 1) {
        out[0] = sub(mul(slope, 1.5), mul(out[1], 0.5));
    }
    if let Some(slope) = slope(n - 2, n - 1) {
        out[n - 1] = sub(mul(slope, 1.5), mul(out[n - 2], 0.5));
    }
    out
}

fn field_object<'a>(object: &'a dyn Reflect, field: &str) -> Option<&'a dyn Reflect> {
    match object.field(field)? {
        FieldValue::Object(child) => Some(child),
        _ => None,
    }
}

fn leak(key: &str) -> &'static str {
    Box::leak(key.to_string().into_boxed_str())
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn mul(a: Point, by: f64) -> Point {
    [a[0] * by, a[1] * by, a[2] * by]
}

fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: Point) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: Point) -> Option<Point> {
    let length = norm(a);
    (length.is_finite() && length > TINY).then(|| mul(a, 1.0 / length))
}

fn lerp(a: Point, b: Point, fraction: f64) -> Point {
    add(a, mul(sub(b, a), fraction))
}

/// The first `ParametricLineArray` under a representation.
///
/// Not a fixed path: an IJK grid keeps it at `Geometry.Points.ParametricLines`,
/// and nothing says another representation has to agree.
/// Which line of the support a borrowed geometry takes.
///
/// Two spellings, and the first is the schema's own typo — RESQML 2.0.1 writes
/// `LineIndiexOnSupportingRepresentation`, 2.2 corrected it. Neither release
/// knows the other's name for it.
const LINE_INDEX: &[&str] = &[
    "LineIndexOnSupportingRepresentation",
    "LineIndiexOnSupportingRepresentation",
];

fn find_line_array(object: &dyn Reflect) -> Option<&dyn Reflect> {
    if object.type_info().name == "ParametricLineArray" {
        return Some(object);
    }
    for field in object.type_info().fields {
        match object.field(field.name) {
            Some(FieldValue::Object(child)) => {
                if let Some(found) = find_line_array(child) {
                    return Some(found);
                }
            }
            Some(FieldValue::List(items)) => {
                for item in items {
                    if let FieldValue::Object(child) = item
                        && let Some(found) = find_line_array(child)
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

/// The pillar lattice of a column-layer grid, slowest axis first.
///
/// `(Nj + 1) × (Ni + 1)`, because that is what *"the parametric lines follow
/// the indexing of the pillars"* means for a grid, and it is the shape the
/// line selection's axes are matched against. Split coordinate lines are **not**
/// counted: they are extra *columns of points*, not extra lines — a split line
/// is the same geometric line as the pillar it splits, which is exactly why
/// `37c45c00` has 8 coordinate lines and 6 parametric ones.
fn pillar_shape(support: &dyn Reflect) -> Option<Vec<usize>> {
    let axis = |name: &str| {
        values::scalar_i64(support, name)
            .filter(|n| *n >= 0)
            .map(|n| n as usize)
    };
    match (axis("Nj"), axis("Ni")) {
        (Some(nj), Some(ni)) => Some(vec![nj + 1, ni + 1]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line in a **depth** system, which is what every existing test means:
    /// the parameter is a depth and stays one. The elevation reading has its
    /// own test below.
    fn line(kind: Kind, control: Vec<Point>, knots: Option<Vec<f64>>) -> Line {
        Line::new(kind, control, knots, None, Vertical::Down)
    }

    /// A vertical line reads its parameter **through the CRS**, and that clause
    /// is the whole of its geometry.
    ///
    /// > *"Parameter values are interpreted as depth => (X,Y,Z), where the depth
    /// > to Z conversion depends on the vertical CRS direction."*
    /// > — RESQML TRG v2.2 §3.8, identical in the 2.0.1 guide.
    ///
    /// Without it a well is **mirrored about its datum** in an elevation
    /// system: the trajectory and its frames go up, every stored surface of the
    /// same package stays down, and no individual number looks wrong. That is
    /// how it survived until 06/08 — reported on a model whose wells hung above
    /// the horizons built from them.
    #[test]
    fn a_vertical_line_turns_its_depth_into_z_through_the_crs() {
        let control = vec![[275.0, 75.0, 0.0]];
        let knots = Some(vec![0.0, 500.0, 1000.0]);

        let down = Line::new(
            Kind::Vertical,
            control.clone(),
            knots.clone(),
            None,
            Vertical::Down,
        );
        assert_eq!(
            down.at(500.0),
            [275.0, 75.0, 500.0],
            "a depth system stores the depth"
        );

        let up = Line::new(Kind::Vertical, control, knots, None, Vertical::Up);
        assert_eq!(
            up.at(500.0),
            [275.0, 75.0, -500.0],
            "an elevation system stores its negation"
        );
    }

    /// The same for a line left with one control point, whatever it declared.
    ///
    /// That is the parametric **pillar** of an IJK grid, not a well: the
    /// majority of the corpus's parametric grids are read this way, and they
    /// obey the same sentence of the same paragraph.
    #[test]
    fn a_single_control_point_line_follows_the_same_rule() {
        let control = vec![[10.0, 20.0, 0.0]];
        let knots = Some(vec![300.0]);
        let down = Line::new(Kind::Linear, control.clone(), knots.clone(), None, Vertical::Down);
        let up = Line::new(Kind::Linear, control, knots, None, Vertical::Up);
        assert_eq!(down.at(300.0)[2], 300.0);
        assert_eq!(up.at(300.0)[2], -300.0);
    }

    /// And nothing else is touched by it.
    ///
    /// Every other kind carries its z in the control points, where it is
    /// already a coordinate in the local system. Converting there would be the
    /// second flip this whole change exists to avoid.
    #[test]
    fn the_curved_kinds_keep_the_z_their_control_points_carry() {
        let control = vec![[0.0, 0.0, 100.0], [0.0, 0.0, 300.0]];
        let knots = Some(vec![0.0, 200.0]);
        let down = Line::new(Kind::Linear, control.clone(), knots.clone(), None, Vertical::Down);
        let up = Line::new(Kind::Linear, control, knots, None, Vertical::Up);
        assert_eq!(down.at(100.0), up.at(100.0));
        assert_eq!(down.at(100.0)[2], 200.0);
    }

    /// Which reading a CRS asks for, and what "no CRS" means.
    #[test]
    fn the_reading_comes_from_the_crs_and_defaults_to_the_files_own_numbers() {
        use energyml_domain::crs::{Crs, CrsKind, VerticalDirection, VerticalQuantity};
        let crs = |direction| Crs {
            uuid: String::new(),
            kind: "LocalDepth3dCrs",
            x_offset: 0.0,
            y_offset: 0.0,
            z_offset: 0.0,
            areal_rotation: 0.0,
            vertical_direction: direction,
            vertical_quantity: VerticalQuantity::Length,
            horizontal_uom: None,
            vertical_uom: None,
            time_uom: None,
            projected: CrsKind::Absent,
            vertical: CrsKind::Absent,
        };
        assert_eq!(Vertical::of(Some(&crs(VerticalDirection::Down))), Vertical::Down);
        assert_eq!(Vertical::of(Some(&crs(VerticalDirection::Up))), Vertical::Up);
        // Nothing said: the file's own numbers are left alone, which is what
        // the placement layer does too when there is no CRS to place with.
        assert_eq!(Vertical::of(None), Vertical::Down);
    }

    fn close(a: Point, b: Point) -> bool {
        within(a, b, 1e-6)
    }

    /// Equality to a stated tolerance — the derivative checks below measure
    /// with a finite difference, whose own truncation error sets the floor.
    fn within(a: Point, b: Point, tolerance: f64) -> bool {
        (0..3).all(|axis| (a[axis] - b[axis]).abs() < tolerance)
    }

    #[test]
    fn the_kind_table_is_the_schemas_own() {
        // The table was shifted by one until 01/08, which made kind 1 — the
        // *linear* spline — refuse as "a natural cubic spline". Pinned here so
        // it cannot drift back.
        assert_eq!(Kind::of(-1), Some(Kind::Null));
        assert_eq!(Kind::of(0), Some(Kind::Vertical));
        assert_eq!(Kind::of(1), Some(Kind::Linear));
        assert_eq!(Kind::of(2), Some(Kind::NaturalCubic));
        assert_eq!(Kind::of(3), Some(Kind::TangentialCubic));
        assert_eq!(Kind::of(4), Some(Kind::ZLinearCubic));
        assert_eq!(Kind::of(5), Some(Kind::MinimumCurvature));
        assert_eq!(Kind::of(6), None, "an index the standard does not define");
    }

    #[test]
    fn a_single_knot_is_a_vertical_line() {
        // x and y from the control point, z from the parameter. The majority of
        // the corpus's parametric grids are this.
        let line = line(Kind::Vertical, vec![[10.0, 20.0, 300.0]], None);
        assert_eq!(line.at(450.0), [10.0, 20.0, 450.0]);
    }

    #[test]
    fn a_null_line_has_no_points_rather_than_wrong_ones() {
        let line = line(Kind::Null, vec![[10.0, 20.0, 300.0]], None);
        assert!(line.at(450.0).iter().all(|c| c.is_nan()));
    }

    #[test]
    fn a_linear_line_interpolates_between_its_knots() {
        let line = line(
            Kind::Linear,
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 1000.0]],
            Some(vec![0.0, 1000.0]),
        );
        assert!(close(line.at(500.0), [50.0, 0.0, 500.0]));
    }

    /// *"Otherwise, the interpolant in the first or last interval is used as
    /// an extrapolating function"* — and `isBounded`, the flag that would say
    /// otherwise, is in no schema. A pillar of a faulted grid carries
    /// parameters past its control points on purpose: truncating them put
    /// Volve's cells 130 m from where its HDF5 says they are.
    #[test]
    fn a_parameter_past_the_end_extends_the_last_interval() {
        let line = line(
            Kind::Linear,
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 1000.0]],
            Some(vec![0.0, 1000.0]),
        );
        assert!(
            close(line.at(5000.0), [500.0, 0.0, 5000.0]),
            "{:?}",
            line.at(5000.0)
        );
        assert!(
            close(line.at(-5000.0), [-500.0, 0.0, -5000.0]),
            "{:?}",
            line.at(-5000.0)
        );
    }

    /// Three control points, and the extension follows the interval it leaves
    /// by — the last one, not the line from end to end.
    #[test]
    fn the_extension_follows_the_interval_it_leaves_by() {
        let line = line(
            Kind::Linear,
            vec![[0.0, 0.0, 0.0], [0.0, 0.0, 100.0], [100.0, 0.0, 200.0]],
            Some(vec![0.0, 100.0, 200.0]),
        );
        assert!(close(line.at(300.0), [200.0, 0.0, 300.0]), "{:?}", line.at(300.0));
        assert!(
            close(line.at(-100.0), [0.0, 0.0, -100.0]),
            "{:?}",
            line.at(-100.0)
        );
    }

    #[test]
    fn descending_knots_interpolate_the_same_way() {
        // A pillar's parameters run downward when the CRS counts depth.
        let line = line(
            Kind::Linear,
            vec![[0.0, 0.0, 1000.0], [0.0, 0.0, 0.0]],
            Some(vec![1000.0, 0.0]),
        );
        assert!((line.at(500.0)[2] - 500.0).abs() < 1e-9);
    }

    #[test]
    fn nan_padding_is_not_data() {
        // `KnotCount` is the longest line in the array; the short ones are
        // padded. Reading the padding puts NaN in the spline solve and
        // poisons every point on the line.
        let line = line(
            Kind::NaturalCubic,
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 100.0], [f64::NAN; 3]],
            Some(vec![0.0, 100.0, f64::NAN]),
        );
        assert_eq!(line.control.len(), 2, "the padded knot is not a knot");
        assert!(close(line.at(50.0), [5.0, 0.0, 50.0]));
    }

    #[test]
    fn a_natural_cubic_passes_through_its_knots() {
        let control = vec![
            [0.0, 0.0, 0.0],
            [1.0, 2.0, 1.0],
            [2.0, -1.0, 4.0],
            [3.0, 3.0, 9.0],
        ];
        let line = line(
            Kind::NaturalCubic,
            control.clone(),
            Some(vec![0.0, 1.0, 2.0, 3.0]),
        );
        for (index, knot) in [0.0, 1.0, 2.0, 3.0].iter().enumerate() {
            assert!(
                close(line.at(*knot), control[index]),
                "at {knot}: {:?} vs {:?}",
                line.at(*knot),
                control[index]
            );
        }
    }

    #[test]
    fn a_natural_cubic_has_no_curvature_at_its_ends() {
        // What makes it *natural*, and the only end condition the schema
        // states. Measured rather than asserted from the coefficients: a
        // second difference across a small step is the second derivative.
        let line = line(
            Kind::NaturalCubic,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 2.0, 1.0],
                [2.0, -1.0, 4.0],
                [3.0, 3.0, 9.0],
            ],
            Some(vec![0.0, 1.0, 2.0, 3.0]),
        );
        // The second difference of a cubic is `h²·f″ + h³·f‴` exactly, so this
        // estimate carries `h·f‴` of truncation — around 1e-4 here. The
        // tolerance is set above that and the interior curvature, measured the
        // same way, is asserted to be orders larger so the check cannot pass
        // by being blind.
        let step = 1e-5;
        let second = |at: f64, inward: f64| -> Point {
            let (a, b, c) = (
                line.at(at),
                line.at(at + inward * step),
                line.at(at + inward * 2.0 * step),
            );
            let mut out = [0.0; 3];
            for axis in 0..3 {
                out[axis] = (a[axis] - 2.0 * b[axis] + c[axis]) / (step * step);
            }
            out
        };
        for (end, inward) in [(0.0, 1.0), (3.0, -1.0)] {
            let measured = second(end, inward);
            assert!(
                within(measured, [0.0; 3], 1e-2),
                "at {end}: {measured:?} — a natural spline has none"
            );
        }
        let inside = second(1.5, 1.0);
        assert!(
            inside.iter().any(|c| c.abs() > 1.0),
            "the middle of this spline is curved, or the test proves nothing: {inside:?}"
        );
    }

    #[test]
    fn a_natural_cubic_of_two_knots_is_a_straight_segment() {
        // The schema's own special case (1).
        let line = line(
            Kind::NaturalCubic,
            vec![[0.0, 0.0, 0.0], [10.0, 20.0, 30.0]],
            Some(vec![0.0, 1.0]),
        );
        assert!(close(line.at(0.25), [2.5, 5.0, 7.5]));
    }

    #[test]
    fn a_z_linear_cubic_curves_in_x_and_y_and_not_in_z() {
        // "(X,Y) follow a natural cubic spline and Z follows a linear spline."
        // The z of this line is deliberately not linear in the parameter, so a
        // cubic reading of it would show.
        let control = vec![[0.0, 0.0, 0.0], [1.0, 3.0, 1.0], [2.0, 0.0, 8.0]];
        let knots = vec![0.0, 1.0, 2.0];
        let curved = line(Kind::NaturalCubic, control.clone(), Some(knots.clone()));
        let mixed = line(Kind::ZLinearCubic, control, Some(knots));

        let (a, b) = (curved.at(0.5), mixed.at(0.5));
        assert!((a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12);
        assert!((b[2] - 0.5).abs() < 1e-12, "z is linear in the parameter: {b:?}");
        assert!((a[2] - b[2]).abs() > 1e-3, "the z of a full cubic differs: {a:?}");
    }

    #[test]
    fn a_minimum_curvature_segment_is_the_circle_it_describes() {
        // A quarter of the unit circle in the xy plane: from (1,0) heading +y
        // to (0,1) heading −x. Halfway along, a circle is at 45° — and nothing
        // else is. This is the check that says the construction is the arc it
        // claims to be rather than something arc-shaped.
        let line = Line::new(
            Kind::MinimumCurvature,
            vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            Some(vec![0.0, 1.0]),
            Some(vec![[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]]),
            Vertical::Down,
        );
        let half = std::f64::consts::FRAC_1_SQRT_2;
        assert!(close(line.at(0.5), [half, half, 0.0]), "{:?}", line.at(0.5));
        // And every sample is on the circle, not merely the middle one.
        for step in 0..=10 {
            let point = line.at(step as f64 / 10.0);
            let radius = (point[0] * point[0] + point[1] * point[1]).sqrt();
            assert!((radius - 1.0).abs() < 1e-9, "at {step}/10: radius {radius}");
        }
    }

    #[test]
    fn a_minimum_curvature_segment_with_no_dogleg_is_straight() {
        let line = Line::new(
            Kind::MinimumCurvature,
            vec![[0.0, 0.0, 0.0], [0.0, 0.0, 100.0]],
            Some(vec![0.0, 100.0]),
            Some(vec![[0.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
            Vertical::Down,
        );
        assert!(close(line.at(25.0), [0.0, 0.0, 25.0]));
    }

    #[test]
    fn a_curve_still_reaches_its_far_knot_when_the_arc_would_miss_it() {
        // Tangents that no circle through both points can honour. The arc is
        // still drawn, and the closing term puts its end back on the knot —
        // the departure the module documentation names.
        let line = Line::new(
            Kind::MinimumCurvature,
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            Some(vec![0.0, 1.0]),
            Some(vec![[0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            Vertical::Down,
        );
        assert!(close(line.at(1.0), [10.0, 0.0, 0.0]), "{:?}", line.at(1.0));
        assert!(close(line.at(0.0), [0.0, 0.0, 0.0]));
    }

    #[test]
    fn a_tangential_cubic_honours_the_tangents_it_is_given() {
        // The derivative at a knot is the tangent, measured across a small
        // step. A Hermite piece that ignored them would still interpolate, so
        // the endpoints alone would not catch it.
        let line = Line::new(
            Kind::TangentialCubic,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            Some(vec![0.0, 1.0]),
            Some(vec![[0.0, 2.0, 0.0], [0.0, -2.0, 0.0]]),
            Vertical::Down,
        );
        let step = 1e-5;
        let derivative = mul(sub(line.at(step), line.at(0.0)), 1.0 / step);
        assert!(
            within(derivative, [0.0, 2.0, 0.0], 1e-3),
            "{derivative:?} — a forward difference carries `h·f″/2` of error"
        );
    }

    #[test]
    fn a_tangential_cubic_with_no_tangents_falls_back_to_the_natural_spline() {
        // "If a tangent vector is missing, then it is computed in the same
        // fashion as for a natural cubic spline." On three symmetric knots the
        // two readings coincide exactly, which is the sharpest form the claim
        // can take.
        let control = vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]];
        let knots = vec![0.0, 1.0, 2.0];
        let hermite = line(Kind::TangentialCubic, control.clone(), Some(knots.clone()));
        let natural = line(Kind::NaturalCubic, control, Some(knots));
        for step in 0..=8 {
            let t = step as f64 / 4.0;
            assert!(close(hermite.at(t), natural.at(t)), "at {t}");
        }
    }

    #[test]
    fn an_undefined_kind_is_refused_rather_than_guessed() {
        assert_eq!(Kind::of(9), None);
    }
}
