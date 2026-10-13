//! Points borrowed from another representation.
//!
//! `Point3dFromRepresentationLatticeArray` stores no coordinates at all. It
//! names a **supporting representation** and a regular selection of that
//! representation's nodes, and the points are whichever nodes the selection
//! picks. It is how a seismic interpretation is stored: the seismic survey is a
//! lattice of a million nodes, every horizon picked on it is a *map* over the
//! same nodes, and storing the xy of each horizon again would store the survey
//! twenty times.
//!
//! It is the last geometry the corpus contains that this crate refused, and it
//! was refused on purpose at first, because the selection reads two ways and
//! choosing wrong puts a horizon on the wrong nodes without failing.
//!
//! # The two readings
//!
//! `NodeIndicesOnSupportingRepresentation` is an `IntegerLatticeArray`: a
//! `StartValue`, then one `Offset {Value, Count}` per dimension, slowest axis
//! first. `Count` increments of `Value` each, so a dimension with `Count = n`
//! selects `n + 1` nodes.
//!
//! The question is what `Value` counts.
//!
//! - **Flat.** `Value` is a step in the support's *node index*, so the next row
//!   is `+Value` positions along the support's single list of nodes.
//! - **Per axis.** `Value` is a step along *that axis* of the support, to be
//!   multiplied by the support's stride — the number of nodes in the faster
//!   axes — before it becomes an index.
//!
//! Both are in range on the files in the corpus, so no bounds check separates
//! them, and both produce exactly as many points as the representation
//! declares. Only the geometry differs, and only if you look at it.
//!
//! # What settles it
//!
//! **The schema, on the start.** eml's own annotation on `StartValue` reads
//! *"Value representing the global start for the lattice: i.e., iStart +
//! jStart\*ni + kStart\*ni\*nj"*. So the start is a per-axis position
//! **flattened through the support's extents** — the lattice is written in the
//! support's index space, and the encoder is expected to know `ni`.
//!
//! **The standard, on the sibling type.** The entry this one needed is written
//! out in full one section earlier, for
//! `ParametricLineFromRepresentationLatticeArray` — the same selection over a
//! support's *lines* instead of its nodes — and identical word for word in
//! 2.0.1 and 2.2:
//!
//! > BUSINESS RULE: The dimensions of the integer lattice array must be
//! > consistent with the dimensions of the supporting representation.
//! >
//! > BUSINESS RULE: The start value of the integer lattice array must be the
//! > linearized index of the starting line. Example: `iStart + ni * jStart` in
//! > case of a supporting 2D grid.
//!
//! *Dimensions* consistent with *dimensions*: one lattice axis per axis of the
//! support, which is the per-axis reading stated as a rule. Note that this
//! type's own entry says something weaker — the indices must be consistent
//! with the **size** of the support — so the rule is borrowed from the sibling
//! rather than quoted from here. That is why the corpus had to settle it too.
//!
//! **The corpus, on the steps.** Two producers, two releases, and the same
//! answer:
//!
//! | | derived grid | offsets | support |
//! |---|---|---|---|
//! | `030a82f6` (FESAPI, resqml 2.2) | 4 × 2 | `(1, 1)`, `(1, 3)` | 4 × 2 |
//! | `dcbdb683` (Sismage, resqml 2.0.1) | 1163 × 849 | `(1, 848)`, `(1, 1162)` | 1163 × 849 |
//!
//! Read **per axis**, each is an exact bijection: every node of the support is
//! selected once, which is what a horizon picked on a whole seismic survey is.
//! ALWYN's largest index comes out at `848 × 1163 + 1162 = 987 386`, and the
//! support holds `849 × 1163 = 987 387` nodes — the last one, exactly.
//!
//! Read **flat**, ALWYN's 987 387 slots would land on 2 011 distinct nodes:
//! `j + i`, a thin diagonal band, with the horizon's 987 387 depths stacked on
//! two thousand xy positions. It would not fail. It would draw. It would be
//! nothing.
//!
//! So: per axis, combined with the support's strides, and the start flat as the
//! schema says.
//!
//! # Where it refuses
//!
//! If the per-axis reading runs past the end of the support, this does **not**
//! quietly try the other one. A file that needs the other reading is a file
//! this module has understood wrongly, and the failure says so and names both —
//! which is worth more than a picture drawn from the second guess.

use std::cell::Cell;

use energyml_core::value::{FieldValue, Reflect};

use crate::error::{Error, Result};
use crate::points::Point;
use crate::source::ValueSource;
use crate::values::scalar_i64;

/// How deep a chain of borrowed geometry may go.
///
/// A support may itself borrow from a third representation — legal, and one
/// hop is what the corpus does. A support that borrows from something that
/// borrows back is not legal and is also not detectable from one end of the
/// chain, so the depth is capped rather than the cycle chased.
const DEPTH: usize = 8;

thread_local! {
    static BORROWED: Cell<usize> = const { Cell::new(0) };
}

/// The flat indices an `IntegerLatticeArray` selects out of a support of the
/// given shape, in order.
///
/// **The one implementation of the rule**, and it has two callers because the
/// standard states it twice: `Point3dFromRepresentationLatticeArray` selects a
/// support's *nodes*, `ParametricLineFromRepresentationLatticeArray` selects
/// its *lines*, and the two are written in the same sentences. Reading them
/// through two pieces of code would be two chances to read them differently.
///
/// `shape` is the support's extents, slowest axis first.
pub(crate) fn indices_on(lattice: &dyn Reflect, shape: &[usize], at: &str) -> Result<Vec<usize>> {
    let selection = Selection::read(lattice, at)?;
    if selection.steps.len() > shape.len() {
        return Err(Error::malformed(
            at,
            format!(
                "a selection over {} axis/axes of a support that states {}",
                selection.steps.len(),
                shape.len()
            ),
        ));
    }
    let available: usize = shape.iter().product();

    // Strides, slowest axis first: how far one step along an axis moves in the
    // support's flat order. The last axis is the fastest and its stride is 1,
    // which is why a one-dimensional support needs no special case.
    let mut strides = vec![1usize; shape.len()];
    for axis in (0..shape.len().saturating_sub(1)).rev() {
        strides[axis] = strides[axis + 1] * shape[axis + 1];
    }

    let mut out = Vec::with_capacity(selection.total());
    let mut counters = vec![0usize; selection.steps.len()];
    loop {
        let mut index = selection.start;
        for (axis, position) in counters.iter().enumerate() {
            index += *position as i64 * selection.steps[axis].value * strides[axis] as i64;
        }
        if index < 0 || index as usize >= available {
            return Err(Error::malformed(
                at,
                format!(
                    "the selection reaches {index} of a support with {available} — read as a \
                     step along each axis of the support, which is what the standard's own \
                     business rules describe; a file meaning a step in the flat order would \
                     need a different reading and this one has not guessed at it"
                ),
            ));
        }
        out.push(index as usize);

        // Odometer, fastest axis first.
        let mut axis = counters.len();
        loop {
            if axis == 0 {
                return Ok(out);
            }
            axis -= 1;
            counters[axis] += 1;
            if counters[axis] <= selection.steps[axis].count {
                break;
            }
            counters[axis] = 0;
        }
    }
}

/// Evaluate a `Point3dFromRepresentationLatticeArray`.
pub fn read(array: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Vec<Point>> {
    let depth = BORROWED.with(|d| d.get());
    if depth >= DEPTH {
        return Err(Error::malformed(
            at,
            format!("geometry borrowed through more than {DEPTH} representations"),
        ));
    }

    let Some(support_uri) = crate::well::reference_of(array, "SupportingRepresentation") else {
        return Err(Error::malformed(
            at,
            "a lattice over a representation that it does not name",
        ));
    };
    let Some(support) = source.object_at(&support_uri) else {
        return Err(Error::Domain(energyml_domain::Error::Missing(format!(
            "the supporting representation {support_uri}"
        ))));
    };
    let support = support.as_reflect();

    let stated = shape_of(support);

    // The support's own points, evaluated once. This is the expensive line —
    // ALWYN's support is a million-node lattice — and it is why the depth guard
    // is a guard and not a formality.
    let points = BORROWED.with(|d| {
        d.set(depth + 1);
        let result = crate::geometry::of(support, source);
        d.set(depth);
        result
    })?;
    let (points, _) = points.merged();

    // A support that states its shape is checked against what it evaluated to,
    // because every stride is computed from the stated shape and a disagreement
    // there moves every point. A support that states nothing is one flat axis,
    // and its length is what came out.
    let shape = match stated {
        Some(shape) => {
            let declared: usize = shape.iter().product();
            if points.len() != declared {
                return Err(Error::malformed(
                    at,
                    format!(
                        "the support {support_uri} states {shape:?} — {declared} node(s) — and \
                         evaluates to {}",
                        points.len()
                    ),
                ));
            }
            shape
        }
        None => vec![points.len()],
    };

    let Some(lattice) = field_object(array, "NodeIndicesOnSupportingRepresentation") else {
        return Err(Error::malformed(
            at,
            "a lattice over a representation with no `NodeIndicesOnSupportingRepresentation`",
        ));
    };
    Ok(indices_on(lattice, &shape, at)?
        .into_iter()
        .map(|index| points[index])
        .collect())
}

/// How many nodes a `Point3dFromRepresentationLatticeArray` selects, without
/// resolving the support or reading a single coordinate.
///
/// What [`crate::points::count`] needs, and the cheap half of the answer: the
/// selection alone says how many points there will be.
pub fn count(array: &dyn Reflect, at: &str) -> Result<usize> {
    Ok(selection_of(array, at)?.total())
}

/// The shape the selection picks out, slowest axis first.
///
/// One axis per `Offset`, `count + 1` nodes along each — the same shape a
/// stored lattice reports, and asked for the same reason: to tell whether the
/// z dataset laid over these points was written with the axes the other way
/// round — see `points::lattice_dims`, which is what calls this.
pub fn selection_dims(array: &dyn Reflect) -> Result<Vec<usize>> {
    Ok(selection_of(array, "selection")?
        .steps
        .iter()
        .map(|step| step.count + 1)
        .collect())
}

/// The selection inside a `Point3dFromRepresentationLatticeArray`.
fn selection_of(array: &dyn Reflect, at: &str) -> Result<Selection> {
    let Some(lattice) = field_object(array, "NodeIndicesOnSupportingRepresentation") else {
        return Err(Error::malformed(
            at,
            "a lattice over a representation with no `NodeIndicesOnSupportingRepresentation`",
        ));
    };
    Selection::read(lattice, at)
}

/// One axis of the selection.
struct Step {
    /// How far one increment moves **along this axis of the support**.
    value: i64,
    /// How many increments. The axis selects `count + 1` nodes.
    count: usize,
}

/// The regular selection an `IntegerLatticeArray` describes.
struct Selection {
    /// The first node, already flat: the schema defines it as
    /// `iStart + jStart * ni + kStart * ni * nj`.
    start: i64,
    /// One per axis, slowest first — the same order as
    /// [`crate::points`]'s lattice and as `FastestAxisCount` implies.
    steps: Vec<Step>,
}

impl Selection {
    /// From the `IntegerLatticeArray` itself, not from its holder: the two
    /// types that carry one spell the element differently
    /// (`NodeIndicesOnSupportingRepresentation`,
    /// `LineIndicesOnSupportingRepresentation`) and mean exactly the same
    /// thing inside.
    fn read(lattice: &dyn Reflect, at: &str) -> Result<Self> {
        let start = scalar_i64(lattice, "StartValue").unwrap_or(0);

        let offsets = match lattice.field("Offset") {
            Some(FieldValue::List(items)) => items
                .iter()
                .filter_map(|item| item.as_object())
                .collect::<Vec<_>>(),
            Some(FieldValue::Object(one)) => vec![one],
            _ => Vec::new(),
        };
        if offsets.is_empty() {
            return Err(Error::malformed(at, "a node selection with no `Offset`"));
        }

        let mut steps = Vec::with_capacity(offsets.len());
        for offset in offsets {
            let value = scalar_i64(offset, "Value").unwrap_or(1);
            let count = scalar_i64(offset, "Count").unwrap_or(0);
            if count < 0 {
                return Err(Error::malformed(
                    at,
                    format!("an offset repeated {count} time(s)"),
                ));
            }
            steps.push(Step {
                value,
                count: count as usize,
            });
        }
        Ok(Selection { start, steps })
    }

    fn total(&self) -> usize {
        self.steps.iter().map(|step| step.count + 1).product()
    }
}

/// The shape of a representation's nodes, slowest axis first, when it states
/// one.
///
/// A `Grid2dRepresentation` does — `SlowestAxisCount` × `FastestAxisCount` —
/// and that is the only case where a stride means anything. Everything else is
/// one flat list of nodes, and `None` says so: a selection over two axes of
/// something with no second axis has nothing to be combined with, and refusing
/// is the only answer that does not invent a shape.
///
/// # Two places, not two spellings
///
/// The usual version difference in this workspace is a renamed element. This
/// one is a **moved** one: resqml 2.0.1 puts the two counts inside
/// `Grid2dPatch`, and 2.2 lifted them onto the representation. Same names, one
/// level apart, so a reader that looks only at the object finds nothing on a
/// 2.0.1 file — and "nothing" here means "flat list", which is a wrong answer
/// rather than no answer. Both places are looked in, nearest first.
fn shape_of(support: &dyn Reflect) -> Option<Vec<usize>> {
    let holders = [Some(support), field_object(support, "Grid2dPatch")];
    for holder in holders.into_iter().flatten() {
        let axis = |name: &str| scalar_i64(holder, name).filter(|n| *n > 0).map(|n| n as usize);
        match (axis("SlowestAxisCount"), axis("FastestAxisCount")) {
            (Some(slowest), Some(fastest)) => return Some(vec![slowest, fastest]),
            // A grid that names only one axis is a single row of nodes.
            (Some(one), None) | (None, Some(one)) => return Some(vec![one]),
            (None, None) => {}
        }
    }
    // Everything else — a triangulated set, a point set — is a flat list, and
    // `None` says so rather than guessing a length.
    None
}

fn field_object<'a>(object: &'a dyn Reflect, field: &str) -> Option<&'a dyn Reflect> {
    match object.field(field)? {
        FieldValue::Object(child) => Some(child),
        _ => None,
    }
}
