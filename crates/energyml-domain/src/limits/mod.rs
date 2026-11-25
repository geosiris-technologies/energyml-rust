//! The limits between two runs of values along a wellbore: where one unit, one
//! category or one side of a threshold ends and the next begins.
//!
//! Three steps, three kinds of function, and nothing written twice:
//!
//! 1. **Classify** - [`Classification`]: one class per node or per interval of a
//!    frame ([`Support`]), or no class where the file has no value. Four sources:
//!    the stratigraphic units of a frame ([`Classification::of_units`]), the codes
//!    of a categorical property ([`Classification::of_codes`]), a column of the
//!    table such a property names ([`Classification::of_column`]), and a
//!    **threshold** on a continuous property ([`Classification::of_threshold`]):
//!    `value >= threshold` is [`Class::AtOrAbove`], `value < threshold` is
//!    [`Class::Below`] - the words say exactly that, and the equality is on the
//!    upper side. A value that is absent has **no class**: no limit crosses it.
//! 2. **Transitions** - [`transitions`]: the runs of one class and the boundaries
//!    between two adjacent runs, and from each boundary two limits, the **base** of
//!    the run above and the **top** of the run below. **A class that comes back is
//!    never refused**: a pressure between 1 and 5 at the top, above 6 in the
//!    middle and between 1 and 5 at the bottom is three runs, two boundaries and
//!    four limits, and each limit carries its **rank** (the first top of that
//!    class, the second...) and the run it belongs to. A top exists only where
//!    something lies above it, a base only where something lies below: the top of
//!    the first run and the base of the last have no limit, and neither has a run
//!    next to a hole.
//! 3. **Place** - [`Transitions::place`]: the measured depth of a limit. On
//!    intervals, the node that separates the two; on nodes, by the anchor the
//!    caller names ([`Anchor`]): on a node inside the run it bounds, on the
//!    neighbouring node outside it, or - for a threshold - **interpolated**
//!    between the two nodes, linearly in MD. Never across a hole: two nodes further
//!    apart than [`crate::along_md::GapRule`] allows are not bridged.
//!
//! The inputs are [`crate::frame_values::FrameValues`] (and
//! [`crate::well_strata::Strata`] for units): the values and the rows of a table are
//! read there, once. ADR-0051.

use std::collections::HashMap;

use energyml_core::text::plain_number;

use crate::along_md::{GapRule, StepRefusal};
use crate::frame_values::{FrameValues, Support, Value};
use crate::strata_by_md::identity_of;
use crate::table::Cell;
use crate::well_strata::Strata;

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Classes
// ---------------------------------------------------------------------------

/// What a node or an interval is, for the limits: two elements of one class are
/// one run, two of different classes have a limit between them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// A stratigraphic unit, by its identity (its uri, else its uuid, else its title).
    Unit(String),
    /// A code of a categorical property.
    Code(i64),
    /// The value of a column of a category table, as text: `1.5` is `1.5` whatever
    /// width it was stored at.
    Cell(String),
    /// A measure at or above the threshold (`value >= threshold`).
    AtOrAbove,
    /// A measure below the threshold (`value < threshold`).
    Below,
}

impl std::fmt::Display for Class {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Class::Unit(identity) => write!(f, "unit {identity}"),
            Class::Code(code) => write!(f, "code {code}"),
            Class::Cell(text) => f.write_str(text),
            Class::AtOrAbove => f.write_str("at or above the threshold"),
            Class::Below => f.write_str("below the threshold"),
        }
    }
}

/// The threshold of a classification, and the values it was applied to - what the
/// interpolated placement of a limit needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Threshold {
    /// The threshold.
    pub at: f64,
    /// The value of each node or interval, `None` where absent.
    pub values: Vec<Option<f64>>,
}

/// One class per node or interval of a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Classification {
    /// What the classes are placed on.
    pub support: Support,
    /// One per node or interval; `None` where there is no value, and no limit
    /// crosses it.
    pub classes: Vec<Option<Class>>,
    /// Set when the classes are the two sides of a threshold.
    pub threshold: Option<Threshold>,
}

/// Why a suite cannot be classified.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ClassifyRefusal {
    /// The number of classes is not the number of nodes or intervals.
    #[error(
        "{classes} class(es) for {elements} node(s) or interval(s): reading them would shift every limit"
    )]
    Length {
        /// How many classes.
        classes: usize,
        /// How many elements the support has.
        elements: usize,
    },
    /// Codes are asked of a continuous property.
    #[error("`{property}` is continuous: it has no codes; a limit on a measure is a threshold")]
    Continuous {
        /// The property's title.
        property: String,
    },
    /// A threshold is asked of a property that is not continuous.
    #[error("`{property}` is not continuous: a code is not a measure, and has no threshold")]
    NotContinuous {
        /// The property's title.
        property: String,
    },
    /// The value at this element is not a code.
    #[error("the value at element {at} is not a code (a fraction, a truth value or a text)")]
    NotACode {
        /// The element.
        at: usize,
    },
    /// The value at this element is not a number.
    #[error("the value at element {at} is not a number")]
    NotNumbers {
        /// The element.
        at: usize,
    },
    /// The threshold is not a number.
    #[error("a threshold of {at} is not a number")]
    BadThreshold {
        /// The threshold given.
        at: f64,
    },
    /// The property names no table.
    #[error("`{property}` names no category table, so there is no column to classify by")]
    NoTable {
        /// The property's title.
        property: String,
    },
    /// The table has no such column.
    #[error("the table has no column {column}: it has {columns}")]
    NoSuchColumn {
        /// The column asked for.
        column: usize,
        /// How many columns the table has.
        columns: usize,
    },
    /// The column holds several values per row.
    #[error("column `{title}` holds {per_row} values per row: a class is one value")]
    SeveralValuesPerRow {
        /// The column's title.
        title: String,
        /// `ValueCountPerRow`.
        per_row: u64,
    },
    /// The column could not be read.
    #[error("column `{title}` could not be read")]
    Unreadable {
        /// The column's title.
        title: String,
    },
    /// A code names no row of the table.
    #[error("the code {code} at element {at} names no row of the table, so its column has no class for it")]
    NoRow {
        /// The element.
        at: usize,
        /// The code.
        code: String,
    },
}

impl Classification {
    /// A classification of your own: one class per node or interval of `support`.
    ///
    /// # Errors
    ///
    /// [`ClassifyRefusal::Length`] when the numbers differ.
    pub fn new(support: Support, classes: Vec<Option<Class>>) -> Result<Classification, ClassifyRefusal> {
        if classes.len() != support.len() {
            return Err(ClassifyRefusal::Length {
                classes: classes.len(),
                elements: support.len(),
            });
        }
        Ok(Classification {
            support,
            classes,
            threshold: None,
        })
    }

    /// The units of a frame's stratigraphy: one class per interval, the unit that
    /// interval names; an interval that names none, or several, has no class.
    pub fn of_units(strata: &Strata) -> Classification {
        let support = Support::Intervals {
            top: strata.sections.iter().map(|section| section.top).collect(),
            base: strata.sections.iter().map(|section| section.base).collect(),
        };
        let classes = strata
            .sections
            .iter()
            .map(|section| {
                section
                    .unit
                    .and_then(|at| strata.units.get(at))
                    .map(|unit| Class::Unit(identity_of(unit)))
            })
            .collect();
        Classification {
            support,
            classes,
            threshold: None,
        }
    }

    /// The codes of a categorical property: one class per node or interval, the
    /// code it holds. A value that is not an integer is refused, and so is a
    /// continuous property.
    pub fn of_codes(values: &FrameValues) -> Result<Classification, ClassifyRefusal> {
        if values.continuous {
            return Err(ClassifyRefusal::Continuous {
                property: values.title.clone(),
            });
        }
        let mut classes = Vec::with_capacity(values.values.len());
        for (at, value) in values.values.iter().enumerate() {
            classes.push(match value {
                None => None,
                Some(Value::Code(code)) => Some(Class::Code(*code)),
                Some(Value::Number(number)) if number.is_finite() && number.fract() == 0.0 => {
                    Some(Class::Code(*number as i64))
                }
                Some(_) => return Err(ClassifyRefusal::NotACode { at }),
            });
        }
        Classification::new(values.support.clone(), classes)
    }

    /// A column of the table of a categorical property: one class per node or
    /// interval, **the cell of the row its code names** (a facies and its density, a
    /// unit and its age): two codes whose rows hold the same value are one class. A
    /// cell that is absent in the table has no class.
    ///
    /// # Errors
    ///
    /// A property with no table, a column that does not exist, holds several values
    /// per row or cannot be read, a value that is not a code, a code that names no
    /// row.
    pub fn of_column(values: &FrameValues, column: usize) -> Result<Classification, ClassifyRefusal> {
        let Some(categories) = &values.categories else {
            return Err(ClassifyRefusal::NoTable {
                property: values.title.clone(),
            });
        };
        let Some(meta) = categories.columns.get(column) else {
            return Err(ClassifyRefusal::NoSuchColumn {
                column,
                columns: categories.columns.len(),
            });
        };
        let title = meta.title.clone().unwrap_or_else(|| format!("column {column}"));
        if meta.values_per_row > 1 {
            return Err(ClassifyRefusal::SeveralValuesPerRow {
                title,
                per_row: meta.values_per_row,
            });
        }
        if !matches!(categories.lookup.rows.columns.get(column), Some(Some(_))) {
            return Err(ClassifyRefusal::Unreadable { title });
        }
        let mut classes = Vec::with_capacity(values.values.len());
        for (at, value) in values.values.iter().enumerate() {
            let Some(value) = value else {
                classes.push(None);
                continue;
            };
            if value.as_f64().is_none() {
                return Err(ClassifyRefusal::NotACode { at });
            }
            let Some(row) = categories.row_of(value) else {
                return Err(ClassifyRefusal::NoRow {
                    at,
                    code: value.to_string(),
                });
            };
            classes.push(categories.lookup.rows.cell(row, column).map(|cell| {
                Class::Cell(match cell {
                    Cell::Text(text) => text,
                    Cell::Number(number) => plain_number(number),
                    Cell::Boolean(flag) => flag.to_string(),
                })
            }));
        }
        Classification::new(values.support.clone(), classes)
    }

    /// The two sides of a threshold on a continuous property: `value >= threshold`
    /// is [`Class::AtOrAbove`], `value < threshold` is [`Class::Below`], an absent
    /// value has no class.
    pub fn of_threshold(values: &FrameValues, threshold: f64) -> Result<Classification, ClassifyRefusal> {
        if !threshold.is_finite() {
            return Err(ClassifyRefusal::BadThreshold { at: threshold });
        }
        if !values.continuous {
            return Err(ClassifyRefusal::NotContinuous {
                property: values.title.clone(),
            });
        }
        let mut numbers = Vec::with_capacity(values.values.len());
        for (at, value) in values.values.iter().enumerate() {
            numbers.push(match value {
                None => None,
                Some(value) => Some(value.as_f64().ok_or(ClassifyRefusal::NotNumbers { at })?),
            });
        }
        let classes = numbers
            .iter()
            .map(|number| {
                number.map(|number| {
                    if number >= threshold {
                        Class::AtOrAbove
                    } else {
                        Class::Below
                    }
                })
            })
            .collect();
        let mut classification = Classification::new(values.support.clone(), classes)?;
        classification.threshold = Some(Threshold {
            at: threshold,
            values: numbers,
        });
        Ok(classification)
    }
}

// ---------------------------------------------------------------------------
// Transitions
// ---------------------------------------------------------------------------

/// A maximal run of consecutive elements of one class.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// The class of the run.
    pub class: Class,
    /// Its first node or interval.
    pub first: usize,
    /// Its last node or interval, inclusive.
    pub last: usize,
    /// Which run of that class this is, from 1, in depth order: the first, the
    /// second...
    pub rank: usize,
}

/// Where two adjacent runs of different classes meet, between element
/// `runs[above].last` and element `runs[below].first`.
#[derive(Clone, Debug, PartialEq)]
pub struct Boundary {
    /// The run above, by its position in [`Transitions::runs`].
    pub above: usize,
    /// The run below.
    pub below: usize,
}

/// Which end of a run a limit is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    /// The top of a run: something lies above it.
    Top,
    /// The base of a run: something lies below it.
    Base,
}

/// One limit of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    /// A top or a base.
    pub kind: LimitKind,
    /// The class of the run it bounds.
    pub class: Class,
    /// The rank of that run among the runs of its class, from 1: this is the
    /// *n*-th top of the class, or its *n*-th base.
    pub rank: usize,
    /// The run it bounds, by its position in [`Transitions::runs`].
    pub run: usize,
    /// The boundary it lies on, by its position in [`Transitions::boundaries`].
    pub boundary: usize,
}

/// The runs, the boundaries and the limits of a classification.
#[derive(Clone, Debug, PartialEq)]
pub struct Transitions {
    /// What the classes are placed on.
    pub support: Support,
    /// The threshold, when the classes are its two sides.
    pub threshold: Option<Threshold>,
    /// The runs, in depth order.
    pub runs: Vec<Run>,
    /// The boundaries, in depth order.
    pub boundaries: Vec<Boundary>,
    /// The limits, in depth order: at one boundary the base of the run above comes
    /// before the top of the run below.
    pub limits: Vec<Limit>,
}

/// Every switch of a classification, **a class that comes back included**.
///
/// A run is a maximal stretch of elements of one class; an element with no class
/// ends a run and no limit crosses it. Two adjacent runs make a boundary, and a
/// boundary the base of the upper run and the top of the lower. A class met again
/// further down is a new run of that class, with the next rank. Nothing is refused.
pub fn transitions(classification: &Classification) -> Transitions {
    let mut runs: Vec<Run> = Vec::new();
    let mut ranks: HashMap<&Class, usize> = HashMap::new();
    let mut open: Option<usize> = None;
    for (at, class) in classification.classes.iter().enumerate() {
        let Some(class) = class else {
            open = None;
            continue;
        };
        match open {
            Some(run) if runs[run].class == *class => runs[run].last = at,
            _ => {
                let rank = ranks.entry(class).or_insert(0);
                *rank += 1;
                runs.push(Run {
                    class: class.clone(),
                    first: at,
                    last: at,
                    rank: *rank,
                });
                open = Some(runs.len() - 1);
            }
        }
    }
    let mut boundaries = Vec::new();
    let mut limits = Vec::new();
    for above in 0..runs.len().saturating_sub(1) {
        let below = above + 1;
        // Adjacent: nothing between them - a hole would have ended the run.
        if runs[above].last + 1 != runs[below].first {
            continue;
        }
        let boundary = boundaries.len();
        boundaries.push(Boundary { above, below });
        limits.push(Limit {
            kind: LimitKind::Base,
            class: runs[above].class.clone(),
            rank: runs[above].rank,
            run: above,
            boundary,
        });
        limits.push(Limit {
            kind: LimitKind::Top,
            class: runs[below].class.clone(),
            rank: runs[below].rank,
            run: below,
            boundary,
        });
    }
    Transitions {
        support: classification.support.clone(),
        threshold: classification.threshold.clone(),
        runs,
        boundaries,
        limits,
    }
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

/// Where a limit sits among the nodes, when the support is nodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Anchor {
    /// On a node **inside the run it bounds**: the first node of a top, the last
    /// node of a base.
    #[default]
    Inside,
    /// On the node **outside the run it bounds**: for a top, the last node of the
    /// run above; for a base, the first node of the run below.
    Outside,
    /// Between the two nodes, where the threshold is crossed, linearly in MD:
    /// `md = md_i + (s - v_i) / (v_j - v_i) * (md_j - md_i)`. Only for a threshold,
    /// and never over a hole.
    Interpolated,
}

/// Where a placed limit is.
#[derive(Clone, Debug, PartialEq)]
pub enum Spot {
    /// On this node of the frame.
    Node(usize),
    /// Between these two nodes, at this fraction of the way (`0 < t < 1`).
    Between {
        /// The node above.
        lower: usize,
        /// The node below.
        upper: usize,
        /// The fraction of the way from `lower` to `upper`.
        t: f64,
    },
}

/// A limit with its measured depth.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// The measured depth.
    pub md: f64,
    /// Where it comes from.
    pub spot: Spot,
}

/// Why a limit cannot be placed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PlaceRefusal {
    /// An interpolated limit needs a threshold and its values.
    #[error("the classes are not the two sides of a threshold, so there is no crossing to interpolate")]
    NotAThreshold,
    /// The two nodes are further apart than the gap rule allows.
    #[error("the nodes {lower} and {upper} are {gap} apart, wider than the {max} that may be bridged")]
    GapTooWide {
        /// The node above.
        lower: usize,
        /// The node below.
        upper: usize,
        /// The distance between them.
        gap: f64,
        /// The widest interval that may be bridged.
        max: f64,
    },
    /// The spacing of the nodes cannot be judged.
    #[error("the nodes give no spacing to judge a gap by: {0}")]
    Axis(StepRefusal),
}

impl Transitions {
    /// The measured depth of a limit of these transitions.
    ///
    /// On **intervals** the limit is the node that separates the two intervals and
    /// the anchor means nothing. On **nodes**, `anchor` names where it sits
    /// ([`Anchor`]); the interpolated one is refused for classes that are not a
    /// threshold, and over a gap wider than the rule of
    /// [`crate::along_md::GapRule`]. On a crossing that falls exactly on a node
    /// (`v == s`), the limit is that node and no gap is bridged.
    pub fn place(&self, limit: &Limit, anchor: Anchor) -> Result<Placed, PlaceRefusal> {
        let boundary = &self.boundaries[limit.boundary];
        // The element above and the element below the boundary.
        let (i, j) = (self.runs[boundary.above].last, self.runs[boundary.below].first);
        match &self.support {
            Support::Intervals { base, .. } => Ok(Placed {
                md: base[i],
                spot: Spot::Node(j),
            }),
            Support::Nodes { md } => {
                let node = |at: usize| Placed {
                    md: md[at],
                    spot: Spot::Node(at),
                };
                match anchor {
                    Anchor::Inside => Ok(node(match limit.kind {
                        LimitKind::Top => j,
                        LimitKind::Base => i,
                    })),
                    Anchor::Outside => Ok(node(match limit.kind {
                        LimitKind::Top => i,
                        LimitKind::Base => j,
                    })),
                    Anchor::Interpolated => {
                        let Some(threshold) = &self.threshold else {
                            return Err(PlaceRefusal::NotAThreshold);
                        };
                        let (Some(Some(vi)), Some(Some(vj))) =
                            (threshold.values.get(i), threshold.values.get(j))
                        else {
                            return Err(PlaceRefusal::NotAThreshold);
                        };
                        let (vi, vj, s) = (*vi, *vj, threshold.at);
                        // On a node: no crossing between two nodes to bridge.
                        if vi == s {
                            return Ok(node(i));
                        }
                        if vj == s {
                            return Ok(node(j));
                        }
                        let rule = GapRule::of_axis(md).map_err(PlaceRefusal::Axis)?;
                        let gap = md[j] - md[i];
                        if !rule.bridges(gap) {
                            return Err(PlaceRefusal::GapTooWide {
                                lower: i,
                                upper: j,
                                gap,
                                max: rule.max_gap(),
                            });
                        }
                        let t = (s - vi) / (vj - vi);
                        Ok(Placed {
                            md: md[i] + t * gap,
                            spot: Spot::Between {
                                lower: i,
                                upper: j,
                                t,
                            },
                        })
                    }
                }
            }
        }
    }
}
