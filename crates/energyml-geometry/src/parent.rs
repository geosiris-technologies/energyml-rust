//! Representations whose geometry belongs to another object.
//!
//! Two of them, and they are less alike than they look. A **local grid** names
//! a window of its parent and a rule for cutting it up; a **sub-representation**
//! names a list of the parent's elements. The first is a box and the second is
//! a set, and that difference decides everything below - including how little
//! of the parent each can get away with reading.
//!
//! # What a local grid is
//!
//! A refined region of another grid. The child names no points at all - it
//! names a **parent window**, a box of the parent's cells, and a **regrid**
//! saying how many child cells each parent cell is cut into. Its geometry is
//! therefore not read, it is *derived*: the parent's corner-point geometry,
//! subdivided.
//!
//! This crate used to refuse every one of them by name - `an IJK grid with
//! no Geometry of its own` - which was honest and useless. Refusing is the
//! right answer only while the alternative is a guess, and there is no guess
//! here: the construction is arithmetic on numbers the file gives.
//!
//! # Only the window is read
//!
//! A local grid refines a small part of a large model - that is what it is for.
//! Evaluating the parent whole and keeping a corner of it would read a
//! reservoir to draw a well pad. So the parent's `Points` is read **through a
//! hyperslab**: one selection per row of pillars in the window, `(pj + 1)`
//! reads of `(pk + 1) × (pi + 1) × 3` doubles instead of one read of the whole
//! array. On a 200 × 200 × 100 parent refined over 2 × 2 × 5 cells that is
//! 0.02 % of the bytes.
//!
//! The seam already existed: [`ArrayLocation`] carries `start` and `count` per
//! dimension and the HDF5 backend turns them into a selection. Nothing in the
//! storage layer changed for this.
//!
//! An array that cannot be sliced - a lattice, a parametric array, anything
//! computed rather than stored - falls back to being evaluated whole and
//! indexed. That is a correctness fallback, not a fast path, and it is the only
//! case where a local grid costs its parent.
//!
//! # Where it stops, and why
//!
//! **A split coordinate line inside the window is refused by name.** A child
//! node sitting on a pillar that a fault has split has two positions, one per
//! side, and choosing between them is exactly the guess this crate does not
//! make. The corpus's one local grid refines column 0 of *"two faulted sugar
//! cubes"*, whose two split lines both belong to column 1 - so the window is
//! unambiguous, and a window that were not is *named* rather than drawn on the
//! wrong side of the fault. Carrying the split down into the child is real work
//! and wants a file that exercises it.
//!
//! **A K gap inside the window is refused the same way, and for the same
//! reason.** A gap gives the parent one more level of points than it has
//! layer boundaries, so the child node on the boundary between the two layers
//! would have two heights - the base of the layer above, the top of the one
//! below. A gap **outside** the window, or at its edge, is only a shift: the
//! window's first level is the top of its first layer (`ijk::layer_tops`), not
//! the layer's index. Nothing in RESQML says how a child's own `KGaps` would
//! compose with its regrid (TUG 2.2 §9.8.2 is silent), which is why a child
//! carrying them is refused below rather than read.
//!
//! # A sub-representation cannot do the same, and it is worth saying why
//!
//! Because its selection is a **list of indices**, not a box. Nothing says they
//! are contiguous, sorted, or anywhere near each other: a sub-representation is
//! how a file says "these 4 000 cells are the reservoir", and those cells may
//! be scattered through the whole grid. There is no hyperslab that means that.
//!
//! What there *is*, and what this does not yet do, is read the indices' own
//! bounding range in one selection and gather from it - never worse than
//! reading the parent whole, and much better for the common case of a
//! sub-representation that names a contiguous region. It is left undone
//! deliberately: **no package in the corpus contains a sub-representation at
//! all**, so the fast path would be written against nothing, and a gather that
//! is only exercised by its own unit test is how an off-by-one ships. The
//! selection below evaluates the support and picks from it, which is correct
//! and honest about its cost.
//!
//! [`ArrayLocation`]: energyml_storage::array::ArrayLocation

use energyml_core::value::{FieldValue, Reflect, Scalar};
use energyml_storage::array::ArrayLocation;

use crate::error::{Error, Result};
use crate::geometry::{Patch, Topology};
use crate::points::Point;
use crate::source::ValueSource;
use crate::values::scalar_i64;

/// An IJK grid with a parent window and no geometry of its own.
///
/// `dimensions` are the child's own `Ni`, `Nj`, `Nk`.
pub(crate) fn ijk_grid(
    representation: &dyn Reflect,
    source: &dyn ValueSource,
    dimensions: [usize; 3],
    at: &str,
) -> Result<Vec<Patch>> {
    let [ni, nj, nk] = dimensions;
    let shape = format!("{at} {ni}×{nj}×{nk}");

    let Some(window) = crate::geometry::field_object(representation, "ParentWindow") else {
        return Err(Error::unsupported(
            "an IJK grid with neither a `Geometry` nor a `ParentWindow`",
            shape,
        ));
    };
    // The three window kinds are not interchangeable: a column-layer or cell
    // window identifies its parent elements by explicit index lists, which is a
    // different construction from a regrid and needs a parent this build can
    // read as an unstructured column-layer grid. Named, not approximated.
    let kind = window.type_info().name;
    if !kind.ends_with("IjkParentWindow") {
        return Err(Error::unsupported(kind, format!("{shape}.ParentWindow")));
    }

    // A K gap in the *child* would give it more levels than its regrid
    // describes, and the schema says nothing about how the two compose.
    if crate::geometry::field_object(representation, "KGaps").is_some() {
        return Err(Error::unsupported("a local grid with K gaps of its own", shape));
    }

    let axes = [
        Axis::of(window, source, "IRegrid", ni, &shape)?,
        Axis::of(window, source, "JRegrid", nj, &shape)?,
        Axis::of(window, source, "KRegrid", nk, &shape)?,
    ];

    // The names are `energyml_domain::crs::PARENT_GRID_FIELDS`, shared with the
    // CRS lookup that has to make the same hop: a local grid is in its parent's
    // system, and one list of spellings serves both.
    let Some(parent_uri) = energyml_domain::crs::PARENT_GRID_FIELDS
        .iter()
        .find_map(|field| crate::well::reference_of(window, field))
    else {
        return Err(Error::malformed(
            &shape,
            "a parent window that names no parent grid",
        ));
    };
    let Some(parent) = source.object_at(&parent_uri) else {
        return Err(Error::Domain(energyml_domain::Error::Missing(format!(
            "the parent grid {parent_uri}"
        ))));
    };
    let parent = parent.as_reflect();

    let lattice = Lattice::of(parent, source, &axes, &shape)?;
    build(&axes, &lattice, dimensions)
}

/// One axis of a regrid, resolved to child node positions.
///
/// The regrid is described in cells; what the geometry needs is **nodes**, and
/// in the parent's own continuous coordinate rather than in cells. So the whole
/// axis is reduced to one array: `nodes[n]` is where child node `n` sits,
/// measured in parent cells from the start of the window.
///
/// That single change of variable is what makes refinement, coarsening, a
/// different ratio per interval and non-uniform child weights all one case.
struct Axis {
    /// First parent cell of the window. 0-based on the **parent grid**, which
    /// the schema states outright: *"0-based index for the placement of the
    /// window on the parent grid"*.
    start: usize,
    /// How many parent cells the window spans.
    parents: usize,
    /// Position of each child node, in parent cells from `start`. Length is the
    /// child's dimension on this axis, plus one.
    nodes: Vec<f64>,
}

impl Axis {
    fn of(
        window: &dyn Reflect,
        source: &dyn ValueSource,
        field: &str,
        children: usize,
        at: &str,
    ) -> Result<Self> {
        let Some(regrid) = crate::geometry::field_object(window, field) else {
            return Err(Error::malformed(at, format!("a parent window with no `{field}`")));
        };
        let start = scalar_i64(regrid, "InitialIndexOnParentGrid").unwrap_or(0).max(0) as usize;

        // "If there is a 1:1 correspondence between the parent and child cells,
        // then this object is not needed." One parent cell per child cell, so
        // the nodes are the integers.
        let Some(intervals) = crate::geometry::field_object(regrid, "Intervals") else {
            return Ok(Self {
                start,
                parents: children,
                nodes: (0..=children).map(|n| n as f64).collect(),
            });
        };

        let count = scalar_i64(intervals, "IntervalCount").unwrap_or(0).max(0) as usize;
        let parents_per = interval_counts(intervals, source, "ParentCountPerInterval", at, field)?;
        let children_per = interval_counts(intervals, source, "ChildCountPerInterval", at, field)?;
        if parents_per.len() != count || children_per.len() != count {
            return Err(Error::malformed(
                at,
                format!(
                    "`{field}` declares {count} interval(s), with {} parent count(s) and {} child \
                     count(s)",
                    parents_per.len(),
                    children_per.len()
                ),
            ));
        }

        // Optional: one weight per child cell of the whole axis, "proportional
        // to the relative sizes of child cells within each interval. The
        // weights need not be normalized." Absent means uniform, which the
        // schema also states.
        let weights = match crate::geometry::field_object(intervals, "ChildCellWeights") {
            Some(array) => Some(crate::values::read_doubles(
                array,
                source,
                &format!("{at}.{field}.ChildCellWeights"),
            )?),
            None => None,
        };

        let mut nodes = vec![0.0f64];
        let mut parents = 0usize;
        let mut child = 0usize;
        for interval in 0..count {
            let span = parents_per[interval];
            let cut = children_per[interval];
            if cut == 0 {
                // "If the child grid type is not commensurate with the parent
                // type, then this attribute is ignored by a reader, and its
                // value should be set to null." An IJK child of an IJK parent
                // is commensurate on all three axes, so a null here is the file
                // describing something this construction cannot place.
                return Err(Error::malformed(
                    at,
                    format!("`{field}` interval {interval} cuts {span} parent cell(s) into none"),
                ));
            }
            let slice = weights.as_ref().and_then(|w| w.get(child..child + cut));
            let total: f64 = slice.map(|w| w.iter().sum()).unwrap_or(cut as f64);
            // NaN as well as zero and negative: a weight array read from a
            // file can hold any of the three, and all three divide by nothing.
            if total.is_nan() || total <= 0.0 {
                return Err(Error::malformed(
                    at,
                    format!("`{field}` interval {interval} has child weights summing to {total}"),
                ));
            }
            let base = parents as f64;
            let mut run = 0.0f64;
            for step in 0..cut {
                run += slice.map(|w| w[step]).unwrap_or(1.0);
                nodes.push(base + span as f64 * run / total);
            }
            parents += span;
            child += cut;
        }

        // The schema's own rule, and the one that catches a misread regrid
        // before it silently draws a grid of the wrong size: "the sum of values
        // over all intervals must be equal to the corresponding child grid
        // dimension".
        if child != children {
            return Err(Error::malformed(
                at,
                format!("`{field}` cuts {child} child cell(s) for a dimension of {children}"),
            ));
        }
        Ok(Self {
            start,
            parents,
            nodes,
        })
    }
}

fn interval_counts(
    intervals: &dyn Reflect,
    source: &dyn ValueSource,
    field: &str,
    at: &str,
    axis: &str,
) -> Result<Vec<usize>> {
    let Some(array) = crate::geometry::field_object(intervals, field) else {
        return Err(Error::malformed(at, format!("`{axis}` with no `{field}`")));
    };
    let values = crate::values::read_integers(array, source, &format!("{at}.{axis}.{field}"))?;
    Ok(values.into_iter().map(|v| v.max(0) as usize).collect())
}

/// The parent's pillar rectangle over the window, and nothing else.
///
/// Indexed `(level × (nj + 1) + dj) × (ni + 1) + di`, with `di`, `dj` and the
/// level counted from the window's own origin.
struct Lattice {
    points: Vec<Point>,
    /// Parent cells spanned on I and J - so the pillar counts are these plus
    /// one.
    span: [usize; 2],
}

impl Lattice {
    fn at(&self, di: usize, dj: usize, level: usize) -> Point {
        let [ni, _] = self.span;
        self.points[(level * (self.span[1] + 1) + dj) * (ni + 1) + di]
    }

    fn of(parent: &dyn Reflect, source: &dyn ValueSource, axes: &[Axis; 3], at: &str) -> Result<Self> {
        let ni = dimension(parent, "Ni", at)?;
        let nj = dimension(parent, "Nj", at)?;
        let nk = dimension(parent, "Nk", at)?;

        for (axis, (name, extent)) in axes.iter().zip([("I", ni), ("J", nj), ("K", nk)]) {
            if axis.start + axis.parents > extent {
                return Err(Error::malformed(
                    at,
                    format!(
                        "a window covering parent cells {}..{} on {name}, of a parent {extent} cell(s) wide",
                        axis.start,
                        axis.start + axis.parents
                    ),
                ));
            }
        }

        let Some(geometry) = crate::geometry::field_object(parent, "Geometry") else {
            // A local grid of a local grid is legal, and reads exactly like
            // this one - but nothing in the corpus does it, so it is named
            // rather than recursed into blind.
            return Err(Error::unsupported(
                "a parent grid that has no geometry of its own either",
                at.to_string(),
            ));
        };
        let Some(array) = crate::geometry::field_object(geometry, "Points") else {
            return Err(Error::malformed(at, "a parent geometry with no `Points`"));
        };

        // A fault crossing the window would give a child node two positions.
        // Checked before any point is read, so the refusal costs nothing.
        let pillars = (ni + 1) * (nj + 1);
        let splits = crate::ijk::split_lines(geometry, source, pillars, at)?;
        if let Some(column) = splits.within(
            ni,
            [axes[0].start, axes[1].start],
            [axes[0].parents, axes[1].parents],
        ) {
            return Err(Error::unsupported(
                "a parent window over a split coordinate line",
                format!("{at}, parent column {column}"),
            ));
        }

        // A K gap gives the parent a level that belongs to no layer, so the
        // window's first level is the top of its first layer, not its index.
        // Inside the window it is the fault again, on K: the child's node on
        // that level would have two heights, the base of the layer above and
        // the top of the one below - refused rather than picked. A gap at the
        // window's edge changes nothing but where the window starts.
        let tops = crate::ijk::layer_tops(parent, source, nk, at)?;
        let levels = tops.last().map(|top| top + 2).unwrap_or(nk + 1);
        let (k0, pk) = (axes[2].start, axes[2].parents);
        if let Some(layer) = (k0..(k0 + pk).saturating_sub(1)).find(|&k| tops[k + 1] != tops[k] + 1) {
            return Err(Error::unsupported(
                "a parent window across a K gap",
                format!("{at}, gap after parent layer {layer}"),
            ));
        }

        let origin = [axes[0].start, axes[1].start, tops.get(k0).copied().unwrap_or(k0)];
        let span = [axes[0].parents, axes[1].parents, axes[2].parents];
        let lines = pillars + splits.count;
        // The window's points are the parent's, so they are read in the
        // parent's system - the geometry that names it is right here.
        let vertical = crate::parametric::Vertical::read(geometry, source);
        let points = window_points(array, source, [ni, levels], lines, origin, span, at, vertical)?;
        Ok(Self {
            points,
            span: [span[0], span[1]],
        })
    }
}

/// Read the window's pillars, slicing the file when the file can be sliced.
///
/// `parent` is the parent's `Ni` and its **levels** - `Nk + 1` plus one per K
/// gap - and `origin[2]` a level, not a layer.
///
/// One hyperslab per row of pillars: the pillar index is `j × (Ni + 1) + i`, so
/// a rectangle of pillars is contiguous along `i` and strided along `j`. Rather
/// than teach the storage layer a stride, this issues `(pj + 1)` selections -
/// each one contiguous, each one a single HDF5 read.
#[allow(clippy::too_many_arguments)]
fn window_points(
    array: &dyn Reflect,
    source: &dyn ValueSource,
    parent: [usize; 2],
    lines: usize,
    origin: [usize; 3],
    span: [usize; 3],
    at: &str,
    vertical: crate::parametric::Vertical,
) -> Result<Vec<Point>> {
    let [ni, levels] = parent;
    let [i0, j0, k0] = origin;
    let [pi, pj, pk] = span;
    let wanted = (pk + 1) * (pj + 1) * (pi + 1);

    if let Some(location) = sliceable(array, source, levels, lines) {
        let mut points = Vec::with_capacity(wanted);
        for dj in 0..=pj {
            let first = (j0 + dj) * (ni + 1) + i0;
            let mut part = location.clone();
            part.start = vec![k0 as i64, first as i64, 0];
            part.count = vec![(pk + 1) as i64, (pi + 1) as i64, 3];
            let data = source.read(&part)?;
            let values = data.to_f64();
            if values.len() != (pk + 1) * (pi + 1) * 3 {
                return Err(Error::malformed(
                    at,
                    format!(
                        "a window row of {} value(s) where {} were selected",
                        values.len(),
                        (pk + 1) * (pi + 1) * 3
                    ),
                ));
            }
            points.push(values);
        }
        // The reads came back one j-row at a time; the caller wants them laid
        // out level-major.
        let mut out = vec![[0.0f64; 3]; wanted];
        for (dj, values) in points.iter().enumerate() {
            for level in 0..=pk {
                for di in 0..=pi {
                    let from = (level * (pi + 1) + di) * 3;
                    out[(level * (pj + 1) + dj) * (pi + 1) + di] =
                        [values[from], values[from + 1], values[from + 2]];
                }
            }
        }
        return Ok(out);
    }

    // Computed rather than stored, so there is nothing to slice: evaluate it
    // and take the corner. Correct, and the one case where a local grid costs
    // as much as its parent.
    let whole = crate::points::read(array, source, &format!("{at}.Geometry.Points"), vertical)?;
    if whole.len() != levels * lines {
        return Err(Error::malformed(
            at,
            format!(
                "a parent holding {} point(s) for {levels} level(s) of {lines} line(s)",
                whole.len()
            ),
        ));
    }
    let mut out = Vec::with_capacity(wanted);
    for level in k0..=k0 + pk {
        for dj in 0..=pj {
            for di in 0..=pi {
                let pillar = (j0 + dj) * (ni + 1) + (i0 + di);
                out.push(whole[level * lines + pillar]);
            }
        }
    }
    Ok(out)
}

/// The one location of a points array that is a plain `(levels, lines, 3)`
/// dataset, and so can be read a piece at a time.
fn sliceable(
    array: &dyn Reflect,
    source: &dyn ValueSource,
    levels: usize,
    lines: usize,
) -> Option<ArrayLocation> {
    let name = array.type_info().name;
    if !(name.ends_with("ExternalArray") || name.ends_with("Hdf5Array")) {
        return None;
    }
    let locations = source.locate(array);
    // Several parts means the logical array is spread over more than one
    // dataset, and a selection would have to be split across them. Whole.
    let [location] = locations.as_slice() else {
        return None;
    };
    let metadata = source.metadata(location).ok()?;
    let dimensions: Vec<usize> = metadata.dimensions.iter().map(|d| (*d).max(0) as usize).collect();
    // A writer is free to flatten the whole thing to one dimension, and then
    // the rectangle is not expressible as one contiguous run.
    (dimensions == [levels, lines, 3]).then(|| location.clone())
}

/// Subdivide the window into the child's cells.
///
/// A child node sits at a **fractional pillar position** `(u, v)` and a
/// fractional level `w`, all measured in parent cells from the window's origin.
/// Its coordinates are the parent's lattice interpolated there: bilinear across
/// the four pillars around it, then linear between the two levels.
///
/// Interpolating in *pillar* space rather than cell space is what keeps the
/// child watertight. Placing each child cell inside its parent cell separately
/// would give a node on a parent-cell boundary one position per side, and they
/// agree only when the parent's pillars happen to be straight - so the grid
/// would come apart wherever the parent is faulted or curved, by an amount too
/// small to see and large enough to leak.
fn build(axes: &[Axis; 3], lattice: &Lattice, dimensions: [usize; 3]) -> Result<Vec<Patch>> {
    let [ni, nj, nk] = dimensions;
    let mut points = Vec::with_capacity((ni + 1) * (nj + 1) * (nk + 1));
    for w in &axes[2].nodes {
        for v in &axes[1].nodes {
            for u in &axes[0].nodes {
                points.push(interpolate(lattice, *u, *v, *w));
            }
        }
    }

    let plane = (ni + 1) * (nj + 1);
    let row = ni + 1;
    let mut cells = Vec::with_capacity(ni * nj * nk);
    for k in 0..nk {
        for j in 0..nj {
            for i in 0..ni {
                let corner =
                    |dk: usize, dj: usize, di: usize| ((k + dk) * plane + (j + dj) * row + i + di) as u32;
                let node = [
                    corner(0, 0, 0),
                    corner(0, 0, 1),
                    corner(0, 1, 1),
                    corner(0, 1, 0),
                    corner(1, 0, 0),
                    corner(1, 0, 1),
                    corner(1, 1, 1),
                    corner(1, 1, 0),
                ];
                cells.push(crate::ijk::oriented(node, &points));
            }
        }
    }

    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points,
        topology: Topology::Hexahedra {
            cells,
            dimensions: [ni, nj, nk],
            // A local grid numbers its own cells, and this builder keeps every
            // one of them: a position is the index.
            index: Vec::new(),
        },
        closed: Vec::new(),
    }])
}

/// The parent lattice at a fractional position, in parent cells from the
/// window's origin.
fn interpolate(lattice: &Lattice, u: f64, v: f64, w: f64) -> Point {
    let [pi, pj] = lattice.span;
    // A node exactly on the far face lands on the last pillar with a fraction
    // of zero rather than one past the end.
    let split = |value: f64, extent: usize| {
        let cell = (value.floor().max(0.0) as usize).min(extent.saturating_sub(1));
        (cell, value - cell as f64)
    };
    let (i, a) = split(u, pi);
    let (j, b) = split(v, pj);
    let (k, c) = split(w, lattice.points.len() / ((pi + 1) * (pj + 1)) - 1);

    let mut out = [0.0f64; 3];
    for (dk, wk) in [(0usize, 1.0 - c), (1, c)] {
        for (dj, wj) in [(0usize, 1.0 - b), (1, b)] {
            for (di, wi) in [(0usize, 1.0 - a), (1, a)] {
                let weight = wi * wj * wk;
                if weight == 0.0 {
                    continue;
                }
                let point = lattice.at(i + di, j + dj, k + dk);
                for axis in 0..3 {
                    out[axis] += point[axis] * weight;
                }
            }
        }
    }
    out
}

/// A `SubRepresentation`: elements picked out of another representation.
///
/// # The two shapes, which are genuinely different
///
/// RESQML **2.0.1** puts one `SupportingRepresentation` on the object and lets
/// each patch carry one or two `ElementIndices`, each with its *own*
/// `IndexableElement`. RESQML **2.2** moved the `IndexableElement` up onto the
/// object and the `SupportingRepresentation` down onto each patch - so in 2.2 a
/// single sub-representation can draw from several supports, and in 2.0.1 it
/// cannot. Both are read here; neither is translated into the other, because
/// what each can express is not the same.
///
/// # What is refused
///
/// **Two `ElementIndices` on one patch.** That is 2.0.1's way of listing pairs -
/// faces *of* cells, nodes *of* faces - and which member of the pair carries
/// the geometry is not something the file says. Drawing the first and ignoring
/// the second would be a guess with no symptom.
///
/// **Any element other than nodes, cells or triangles.** An edge, a column, a
/// face-per-cell or a subnode has no geometry until the support's topology for
/// *that* element is built, and this build does not build them. Named, one
/// element kind at a time, rather than returned empty.
pub(crate) fn sub_representation(
    representation: &dyn Reflect,
    source: &dyn ValueSource,
) -> Result<Vec<Patch>> {
    let at = "SubRepresentation";
    // 2.2 carries these on the object, 2.0.1 on the patch. Read both here and
    // let each patch prefer its own.
    let object_element = text_of(representation, "IndexableElement");
    // **A convention that overrides the schema element, and it is written down.**
    // TUG §4.4.1 describes how a sub-representation draws from *several*
    // supporting representations in a version whose schema allows one: an
    // `ExtraMetadata` entry per support, keyed `SupportingRepresentation` and
    // **ordered**, plus a discrete property giving each element its support's
    // index. And the guide is explicit about what that does to the field read
    // below: *"In this case, the standard (non extrametadata) subrepresentation
    // element named `SupportingRepresentation` will be ignored."*
    //
    // Nothing here resolves that convention, so meeting it is a **named
    // refusal**. Reading the schema element anyway would attach every element to
    // one support the guide says to ignore - a sub-representation drawn whole,
    // from the wrong object, with nothing to say so.
    if let Some(count) = extra_supports(representation)
        && count > 0
    {
        return Err(Error::unsupported(
            "SupportingRepresentation in ExtraMetadata",
            format!(
                "this sub-representation names {count} support(s) through the ExtraMetadata \
                 convention of the Technical Usage Guide §4.4.1, which then replaces the schema's \
                 own `SupportingRepresentation`"
            ),
        ));
    }

    let object_support = crate::well::reference_of(representation, "SupportingRepresentation");

    let patches = list_of(representation, "SubRepresentationPatch");
    if patches.is_empty() {
        return Err(Error::malformed(at, "a sub-representation with no patch"));
    }

    let mut out = Vec::with_capacity(patches.len());
    let mut offset = 0usize;
    for (index, patch) in patches.iter().enumerate() {
        let where_ = format!("{at}.SubRepresentationPatch[{index}]");
        let elements = list_of(*patch, "ElementIndices");
        if elements.len() > 1 {
            return Err(Error::unsupported(
                "a sub-representation listing pairs of elements",
                where_,
            ));
        }
        let (element, indices) = match elements.first() {
            // 2.0.1: the kind and the indices are both inside `ElementIndices`.
            Some(one) => (
                text_of(*one, "IndexableElement"),
                crate::geometry::field_object(*one, "Indices"),
            ),
            // 2.2: the kind is on the object, the indices on the patch.
            None => (
                object_element.clone(),
                crate::geometry::field_object(*patch, "Indices"),
            ),
        };
        let Some(element) = element else {
            return Err(Error::malformed(&where_, "a patch naming no indexable element"));
        };
        let Some(element) = energyml_domain::property::IndexableElement::parse(&element) else {
            return Err(Error::malformed(
                &where_,
                format!("`{element}` is not an indexable element"),
            ));
        };
        let Some(indices) = indices else {
            return Err(Error::malformed(&where_, "a patch with no `Indices`"));
        };
        let indices = crate::values::read_integers(indices, source, &where_)?;

        let Some(support_uri) =
            crate::well::reference_of(*patch, "SupportingRepresentation").or_else(|| object_support.clone())
        else {
            return Err(Error::malformed(
                &where_,
                "a patch that names no supporting representation",
            ));
        };
        let Some(support) = source.object_at(&support_uri) else {
            return Err(Error::Domain(energyml_domain::Error::Missing(format!(
                "the supporting representation {support_uri}"
            ))));
        };

        let mut patch = select(support.as_reflect(), element, &indices, source, &where_)?;
        patch.index = index;
        patch.node_offset = offset;
        offset += patch.points.len();
        out.push(patch);
    }
    Ok(out)
}

/// The chosen elements of a support, as a patch of its own.
fn select(
    support: &dyn Reflect,
    element: energyml_domain::property::IndexableElement,
    indices: &[i64],
    source: &dyn ValueSource,
    at: &str,
) -> Result<Patch> {
    use energyml_domain::property::IndexableElement::*;

    // The support's own count for this element, which is what the indices are
    // indices *into*. Checking against it turns an index that means nothing
    // into a message rather than a panic or a silent wrong node.
    let available = energyml_domain::property::element_count(support, element)? as i64;
    if let Some(bad) = indices.iter().find(|i| **i < 0 || **i >= available) {
        return Err(Error::malformed(
            at,
            format!("an index of {bad} into {available} `{element}` of the support"),
        ));
    }

    let geometry = crate::geometry::of(support, source)?;
    let (points, triangles) = geometry.merged();

    match element {
        Nodes => Ok(Patch {
            index: 0,
            node_offset: 0,
            points: indices.iter().map(|i| points[*i as usize]).collect(),
            topology: Topology::Points,
            closed: Vec::new(),
        }),
        Triangles => {
            let chosen: Vec<[u32; 3]> = indices.iter().map(|i| triangles[*i as usize]).collect();
            Ok(compact(points, chosen))
        }
        Cells => {
            let cells: Vec<[u32; 8]> = geometry
                .patches
                .iter()
                .flat_map(|patch| match &patch.topology {
                    Topology::Hexahedra { cells, .. } => cells.clone(),
                    _ => Vec::new(),
                })
                .collect();
            if cells.len() as i64 != available {
                return Err(Error::malformed(
                    at,
                    format!(
                        "a support declaring {available} cell(s) and drawing {}",
                        cells.len()
                    ),
                ));
            }
            let chosen: Vec<[u32; 8]> = indices.iter().map(|i| cells[*i as usize]).collect();
            let count = chosen.len();
            let mut patch = compact_cells(points, chosen);
            // A sub-representation of cells is not an IJK box any more, so it
            // states its extent as a flat run rather than claiming dimensions
            // it does not have.
            patch.topology = match patch.topology {
                Topology::Hexahedra { cells, .. } => Topology::Hexahedra {
                    cells,
                    dimensions: [count, 1, 1],
                    // **The supporting grid's numbering does not survive here.**
                    // A sub-representation numbers *its own* elements from zero,
                    // and that is what a property on it lines up against;
                    // carrying the support's indices forward would hand a caller
                    // numbers from another object.
                    index: Vec::new(),
                },
                other => other,
            };
            Ok(patch)
        }
        other => Err(Error::unsupported(
            "a sub-representation of an element with no geometry here",
            format!("{at}, `{other}`"),
        )),
    }
}

/// Keep only the points the chosen triangles use, and renumber them.
///
/// Without this a sub-representation of six triangles carries every point of a
/// two-million-point surface, which is the opposite of what selecting them was
/// for.
fn compact(points: Vec<Point>, triangles: Vec<[u32; 3]>) -> Patch {
    let mut map = std::collections::HashMap::new();
    let mut kept = Vec::new();
    let mut out = Vec::with_capacity(triangles.len());
    for triangle in triangles {
        let mut renumbered = [0u32; 3];
        for (slot, node) in triangle.iter().enumerate() {
            let next = map.len() as u32;
            let index = *map.entry(*node).or_insert_with(|| {
                kept.push(points[*node as usize]);
                next
            });
            renumbered[slot] = index;
        }
        out.push(renumbered);
    }
    Patch {
        index: 0,
        node_offset: 0,
        points: kept,
        topology: Topology::Triangles(out),
        closed: Vec::new(),
    }
}

/// The same, for hexahedra.
fn compact_cells(points: Vec<Point>, cells: Vec<[u32; 8]>) -> Patch {
    let mut map = std::collections::HashMap::new();
    let mut kept = Vec::new();
    let mut out = Vec::with_capacity(cells.len());
    for cell in cells {
        let mut renumbered = [0u32; 8];
        for (slot, node) in cell.iter().enumerate() {
            let next = map.len() as u32;
            let index = *map.entry(*node).or_insert_with(|| {
                kept.push(points[*node as usize]);
                next
            });
            renumbered[slot] = index;
        }
        out.push(renumbered);
    }
    let count = out.len();
    Patch {
        index: 0,
        node_offset: 0,
        points: kept,
        topology: Topology::Hexahedra {
            cells: out,
            dimensions: [count, 1, 1],
            // As above: the selection numbers its own cells from zero.
            index: Vec::new(),
        },
        closed: Vec::new(),
    }
}

fn text_of(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)? {
        FieldValue::Scalar(scalar) => scalar.as_str().map(str::to_string),
        _ => None,
    }
}

fn list_of<'a>(object: &'a dyn Reflect, field: &str) -> Vec<&'a dyn Reflect> {
    match object.field(field) {
        Some(FieldValue::List(items)) => items.iter().filter_map(|item| item.as_object()).collect(),
        Some(FieldValue::Object(one)) => vec![one],
        _ => Vec::new(),
    }
}

fn dimension(object: &dyn Reflect, field: &str, at: &str) -> Result<usize> {
    match scalar_i64(object, field) {
        Some(value) if value > 0 => Ok(value as usize),
        Some(value) => Err(Error::malformed(
            at,
            format!("a parent whose `{field}` is {value}"),
        )),
        None => Err(Error::malformed(at, format!("a parent with no `{field}`"))),
    }
}

/// How many supports the `ExtraMetadata` convention of TUG §4.4.1 names.
///
/// `None` when the object carries no extra metadata at all, which is the
/// ordinary case and costs one field lookup.
///
/// The key is matched exactly - `SupportingRepresentation` - because that is
/// what the guide spells, and a looser match would catch the per-patch keys
/// (`SubRepresentationPatch[…]/ElementIndices/SupportingRepresentationIndex`)
/// and refuse objects that only carry those.
fn extra_supports(object: &dyn Reflect) -> Option<usize> {
    let FieldValue::List(items) = object.field("ExtraMetadata")? else {
        return None;
    };
    Some(
        items
            .iter()
            .filter_map(|item| item.as_object())
            .filter(|pair| {
                matches!(
                    pair.field("Name"),
                    Some(FieldValue::Scalar(Scalar::Str(name))) if name == "SupportingRepresentation"
                )
            })
            .count(),
    )
}
