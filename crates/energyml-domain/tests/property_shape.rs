//! The shape of a property's values, a pick of components, and the reduction of
//! one dimension.
//!
//! Sources: RESQML TRG v2.2 §7.1 `AbstractProperty` p. 222 ("First (left) given
//! dimension is slowest and last (right) given dimension is fastest",
//! `LabelPerComponent`, `RealizationIndices`) and §7 figure p. 221; RESQML
//! Technical Usage Guide v2.2 §6.6.3 "Indexing" p. 65 (time, then elements),
//! §6.6.3.1 (IJK cells `[nk][nj][ni]`), §4.2.2 p. 39 (the 1D order is the same
//! whatever the grouping of the dimensions); EML TRG v2.3 p. 296
//! (`TimeOrIntervalSeries.UseInterval`). Every object is built here, in memory.

use energyml_domain::property::shape::{
    ComponentPick, ComponentRefusal, CountFit, ElementAxes, ElementShape, PropertyShape, Reduction, ShapeRefusal,
    TimeAxis, element_axes,
};
use energyml_domain::property::{self, Discrepancy, Instant};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;

const UUID: &str = "00000000-0000-0000-0000-0000000000a1";
const SERIES: &str = "00000000-0000-0000-0000-0000000000a2";

fn citation() -> eml23::Citation {
    eml23::Citation {
        title: "t".into(),
        originator: "o".into(),
        creation: "2026-11-14T00:00:00Z".into(),
        format: "f".into(),
        ..Default::default()
    }
}

fn continuous(counts: Vec<i64>) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: UUID.into(),
        citation: citation(),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: counts,
        ..Default::default()
    }
}

fn series_reference() -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: SERIES.into(),
        qualified_type: "eml23.TimeSeries".into(),
        title: "series".into(),
        ..Default::default()
    }
}

fn shape(dims: &[u64]) -> ElementShape {
    ElementShape::of(dims, &[]).expect("a valid shape")
}

/// Equality that reads two `NaN` as equal.
fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y || (x.is_nan() && y.is_nan()))
}

fn line(len: usize) -> Vec<f64> {
    (0..len).map(|i| i as f64).collect()
}

// ---------------------------------------------------------------------------
// F1: label_per_component and instant
// ---------------------------------------------------------------------------

/// Break it: read the field `"LabelPerComponents"` (a misspelt name); the test sees `[]`.
#[test]
fn label_per_component_is_read() {
    // RESQML TRG 2.2 §7.1 p. 222: labels "in a linearized way".
    let mut object = continuous(vec![3]);
    object.label_per_component = vec!["a".into(), "b".into(), "c".into()];
    let read = property::read(&object).expect("a property that reads");
    assert_eq!(read.label_per_component, vec!["a", "b", "c"]);
}

/// Break it: read `Time` in `read_time`, or ignore it: the test sees `None` or a `time`.
#[test]
fn a_2_2_time_is_read_as_an_instant() {
    // RESQML TRG 2.2 §7.1 p. 222 (`Time`); a series is another field.
    let mut object = continuous(vec![]);
    object.time = Some(eml23::GeologicTime {
        date_time: "2030-01-01T00:00:00Z".into(),
        age_offset_attribute: None,
    });
    let read = property::read(&object).expect("a property that reads");
    assert_eq!(
        read.instant,
        Some(Instant {
            date_time: "2030-01-01T00:00:00Z".into(),
            age_offset: None,
        })
    );
    assert_eq!(read.time, None, "an instant is not an index into a series");
}

// ---------------------------------------------------------------------------
// F2: ElementShape
// ---------------------------------------------------------------------------

/// Break it: compute the strides from the first dimension.
#[test]
fn the_first_given_dimension_is_the_slowest() {
    // RESQML TRG 2.2 §7.1 p. 222.
    let shape = shape(&[3, 2]);
    assert_eq!(shape.ravel(&[1, 0]), Some(2));
    assert_eq!(shape.ravel(&[0, 1]), Some(1));
    assert_eq!(shape.ravel(&[2, 1]), Some(5));
}

/// Break it: unravel the first dimension first.
#[test]
fn value_vector_index_unravels_on_the_shape() {
    // RESQML TRG 2.2 §7.1 p. 222 (the linear order of the components).
    let shape = shape(&[3, 2]);
    assert_eq!(shape.unravel(5), Some(vec![2, 1]));
    assert_eq!(shape.unravel(6), None);
}

/// Break it: take `%` by `dims[0]` everywhere.
#[test]
fn ravel_and_unravel_are_inverse() {
    let shape = shape(&[2, 3, 4]);
    for i in 0..24 {
        let at = shape.unravel(i).expect("inside");
        assert_eq!(shape.ravel(&at), Some(i));
    }
}

/// Break it: accept any count of labels.
#[test]
fn labels_must_count_the_product() {
    // RESQML TRG 2.2 §7.1 p. 222: "in a linearized way".
    let labels = |n: usize| (0..n).map(|i| format!("c{i}")).collect::<Vec<_>>();
    let ok = ElementShape::of(&[3, 2], &labels(6)).expect("six labels for six components");
    assert_eq!(ok.label_of(&[2, 1]), Some("c5"));
    assert_eq!(
        ElementShape::of(&[3, 2], &labels(5)),
        Err(ShapeRefusal::LabelCount { labels: 5, expected: 6 })
    );
}

/// Break it: ignore the 0.
#[test]
fn a_zero_dimension_is_refused() {
    // Properties.xsd: `ValueCountPerIndexableElement` has `minInclusive = 1`.
    assert_eq!(
        ElementShape::of(&[81, 0], &[]),
        Err(ShapeRefusal::ZeroDimension { at: 1 })
    );
}

/// Break it: `<=` instead of `<`.
#[test]
fn a_component_past_its_axis_is_refused() {
    let shape = shape(&[3, 2]);
    assert_eq!(
        shape.flat_component(&[3, 0]),
        Err(ComponentRefusal::OutOfRange { axis: 0, at: 3, len: 3 })
    );
    assert_eq!(
        shape.flat_component(&[1]),
        Err(ComponentRefusal::Rank { axes: 2, given: 1 })
    );
}

// ---------------------------------------------------------------------------
// F3: ComponentPick
// ---------------------------------------------------------------------------

/// Break it: do not replace `at[axis]`.
#[test]
fn along_reads_one_line_of_the_matrix() {
    // RESQML TRG 2.2 §7.1 p. 222: value = 10 x i + j on a `[3, 2]` shape.
    let shape = shape(&[3, 2]);
    let row = [0.0, 1.0, 10.0, 11.0, 20.0, 21.0];
    let pick = ComponentPick::Along {
        axis: 0,
        at: vec![0, 1],
    };
    assert_eq!(pick.select(&shape, &row).expect("a line"), vec![1.0, 11.0, 21.0]);
}

/// Break it: count the `NaN` as 0.
#[test]
fn a_reduction_ignores_non_finite_values() {
    let shape = shape(&[3]);
    let mean = ComponentPick::Reduce(Reduction::Mean);
    assert_eq!(mean.select(&shape, &[1.0, f64::NAN, 3.0]).expect("a mean"), vec![2.0]);
    assert!(same(
        &mean.select(&shape, &[f64::NAN; 3]).expect("a mean"),
        &[f64::NAN]
    ));
}

/// Break it: ignore the `NaN` in the norm.
#[test]
fn a_norm_needs_a_vector_and_every_component() {
    let norm = ComponentPick::Reduce(Reduction::Norm);
    assert_eq!(
        norm.select(&shape(&[3, 2]), &line(6)),
        Err(ComponentRefusal::NotAVector { rank: 2 })
    );
    let vector = shape(&[3]);
    assert_eq!(norm.select(&vector, &[3.0, 4.0, 0.0]).expect("a norm"), vec![5.0]);
    assert!(same(
        &norm.select(&vector, &[3.0, f64::NAN, 0.0]).expect("a norm"),
        &[f64::NAN]
    ));
}

// ---------------------------------------------------------------------------
// F4: PropertyShape
// ---------------------------------------------------------------------------

/// Break it: put the components in front of the elements.
#[test]
fn the_components_are_the_fastest_dimension() {
    // Usage Guide 2.2 §4.2.2 p. 39 and §6.6.3 p. 65.
    let object = continuous(vec![3, 2]);
    let read = property::read(&object).expect("reads");
    let shape = PropertyShape::of(&read, ElementAxes::Ordered(vec![4]), None, None).expect("a shape");
    assert_eq!(shape.declared_dims(), vec![4, 3, 2]);
    assert_eq!(shape.value_count(), 24);
}

/// Break it: accept any product that is equal (`[6, 4]` must be refused: the product is 24 but 6 regroups no run).
#[test]
fn a_flattened_store_is_the_same_data() {
    // Usage Guide 2.2 §4.2.2 p. 39.
    let object = continuous(vec![3, 2]);
    let read = property::read(&object).expect("reads");
    let shape = PropertyShape::of(&read, ElementAxes::Ordered(vec![4]), None, None).expect("a shape");
    assert_eq!(shape.layout(&[24]).expect("flat").groups, vec![0..3]);
    assert_eq!(shape.layout(&[4, 6]).expect("regrouped").groups, vec![0..1, 1..3]);
    assert_eq!(
        shape.layout(&[4, 3, 2]).expect("as declared").groups,
        vec![0..1, 1..2, 2..3]
    );
    for wrong in [vec![4u64, 5], vec![6, 4], vec![]] {
        assert!(
            matches!(shape.layout(&wrong), Err(ShapeRefusal::StoredMismatch { .. })),
            "{wrong:?} is not a regrouping of [4, 3, 2]"
        );
    }
}

/// Break it: `[ni, nj, nk]`.
#[test]
fn ijk_cells_are_k_then_j_then_i() {
    // Usage Guide 2.2 §6.6.3.1 p. 65: `[nk][nj][ni]`.
    let grid = resqml22::IjkGridRepresentation {
        ni: 4,
        nj: 3,
        nk: 2,
        ..Default::default()
    };
    let axes = element_axes(&grid, property::IndexableElement::Cells).expect("a lattice of cells");
    assert_eq!(axes, ElementAxes::Ordered(vec![2, 3, 4]));
    let columns = element_axes(&grid, property::IndexableElement::Columns).expect("columns");
    assert_eq!(columns, ElementAxes::Ordered(vec![3, 4]));
}

fn series_property(use_interval: bool) -> resqml22::ContinuousProperty {
    let mut object = continuous(vec![]);
    object.time_or_interval_series = Some(eml23::TimeOrIntervalSeries {
        use_interval,
        time_series: series_reference(),
    });
    object
}

/// Break it: accept `n` steps with `UseInterval` (the last line of the guide's table).
#[test]
fn a_time_dimension_that_contradicts_the_series_is_refused() {
    // Usage Guide 2.2 §6.6.3 p. 65; EML TRG 2.3 p. 296 (`UseInterval`).
    let object = series_property(true);
    let read = property::read(&object).expect("reads");
    let shape = PropertyShape::of(&read, ElementAxes::Ordered(vec![5]), Some(11), None).expect("a shape");
    assert_eq!(shape.time, Some(TimeAxis::of(11, true)));
    assert_eq!(
        shape.layout(&[11, 5]),
        Err(ShapeRefusal::TimeCountMismatch {
            array: 11,
            instants: 11,
            use_interval: true,
        })
    );
    assert!(shape.layout(&[10, 5]).is_ok());
}

/// Break it: suppose 1 step.
#[test]
fn a_series_without_its_instant_count_is_refused() {
    let object = series_property(true);
    let read = property::read(&object).expect("reads");
    assert_eq!(
        PropertyShape::of(&read, ElementAxes::Ordered(vec![5]), None, None),
        Err(ShapeRefusal::TimeNotGiven)
    );
}

/// Break it: make every `TimeRef` a dimension.
#[test]
fn an_inherited_time_index_is_not_a_dimension() {
    // Usage Guide 2.2 §6.6.3 p. 65: the inherited time index "should be ignored".
    let object = energyml_resqml_2_0_1::ContinuousProperty {
        uuid: UUID.into(),
        count: 1,
        time_index: Some(energyml_resqml_2_0_1::TimeIndex {
            index: 3,
            time_series: energyml_eml_2_0::DataObjectReference {
                uuid: SERIES.into(),
                title: "series".into(),
                ..Default::default()
            },
        }),
        ..Default::default()
    };
    let read = property::read(&object).expect("reads");
    assert!(read.time.is_some(), "the index is read");
    let shape = PropertyShape::of(&read, ElementAxes::Ordered(vec![5]), Some(11), None).expect("a shape");
    assert_eq!(shape.time, None);
}

// ---------------------------------------------------------------------------
// F5: the count against the shape
// ---------------------------------------------------------------------------

/// Break it: forget the factor of the realizations.
#[test]
fn a_shape_counts_time_and_realizations() {
    // RESQML TRG 2.2 §7 p. 221: `[RealizationCount][TimeCount][elements][components]`.
    let shape = PropertyShape {
        realizations: Some(2),
        time: Some(TimeAxis::of(3, false)),
        elements: ElementAxes::Unordered(4),
        components: self::shape(&[6]),
    };
    assert_eq!(shape.count_fit(144), CountFit::Fits);
    assert_eq!(shape.count_fit(24), CountFit::OnePerElement { declared: 6 });
}

/// Break it: make `check` return nothing for a `Mismatch`.
#[test]
fn the_check_keeps_its_discrepancy() {
    // The montage of `property_counts_in_check`: a 3 x 4 grid and 7 values.
    let grid = resqml22::Grid2dRepresentation {
        uuid: "00000000-0000-0000-0000-0000000000b1".into(),
        citation: citation(),
        slowest_axis_count: 3,
        fastest_axis_count: 4,
        ..Default::default()
    };
    let object = continuous(vec![1]);
    let read = property::read(&object).expect("reads");
    assert_eq!(
        read.check(&grid, Some(7)),
        vec![Discrepancy::ValueCount {
            element_count: 12,
            per_element: 1,
            expected: 12,
            actual: 7,
        }]
    );
}

// ---------------------------------------------------------------------------
// F14: a reduced dimension
// ---------------------------------------------------------------------------

fn matrix_row() -> Vec<f64> {
    // 10 x i + j on `[3, 2]`.
    vec![0.0, 1.0, 10.0, 11.0, 20.0, 21.0]
}

fn along_reduced(axis: usize, at: Vec<usize>, reduced: Vec<usize>, by: Reduction) -> ComponentPick {
    ComponentPick::AlongReduced { axis, at, reduced, by }
}

/// Break it: reduce along `axis` instead of `reduced` (the test sees `[10, 11]`).
#[test]
fn a_dimension_reduced_by_its_mean() {
    // 01 §4.5 (projection); the order of the dimensions: RESQML TRG 2.2 §7.1 p. 222.
    let shape = shape(&[3, 2]);
    let mean = along_reduced(0, vec![0, 0], vec![1], Reduction::Mean);
    assert_eq!(
        mean.select(&shape, &matrix_row()).expect("a mean"),
        vec![0.5, 10.5, 20.5]
    );
    let max = along_reduced(0, vec![0, 0], vec![1], Reduction::Max);
    assert_eq!(max.select(&shape, &matrix_row()).expect("a max"), vec![1.0, 11.0, 21.0]);
}

/// Break it: count the `NaN` as 0 (`(0 + 11) / 2 = 5.5`).
#[test]
fn a_reduced_dimension_skips_non_finite_values() {
    let shape = shape(&[3, 2]);
    let mean = along_reduced(0, vec![0, 0], vec![1], Reduction::Mean);
    let mut row = matrix_row();
    row[2] = f64::NAN;
    assert!(same(&mean.select(&shape, &row).expect("a mean"), &[0.5, 11.0, 20.5]));
    row[3] = f64::NAN;
    assert!(same(
        &mean.select(&shape, &row).expect("a mean"),
        &[0.5, f64::NAN, 20.5]
    ));
}

/// Break it: reduce one dimension after the other (`(1 + 3.5) / 2`).
#[test]
fn two_reduced_dimensions_are_reduced_together() {
    let shape = shape(&[2, 2, 2]);
    let row = [1.0, f64::NAN, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let pick = along_reduced(0, vec![0, 0, 0], vec![1, 2], Reduction::Mean);
    assert_eq!(
        pick.select(&shape, &row).expect("a mean"),
        vec![8.0 / 3.0, 6.5],
        "the mean of the three finite values of the first block, not a mean of means"
    );
}

/// Break it: accept the literal with nothing reduced.
#[test]
fn nothing_reduced_is_along() {
    assert_eq!(
        ComponentPick::along_reduced(0, vec![0, 1], vec![], Reduction::Mean),
        ComponentPick::Along {
            axis: 0,
            at: vec![0, 1]
        }
    );
    let literal = along_reduced(0, vec![0, 1], vec![], Reduction::Mean);
    assert_eq!(
        literal.check(&shape(&[3, 2])),
        Err(ComponentRefusal::NothingReduced { axis: 0 })
    );
}

/// Break it: remove the `d == axis` check.
#[test]
fn the_shown_dimension_cannot_be_reduced() {
    let shape = shape(&[3, 2]);
    assert_eq!(
        along_reduced(0, vec![0, 0], vec![0], Reduction::Mean).check(&shape),
        Err(ComponentRefusal::ReducedAxis { dim: 0 })
    );
    assert_eq!(
        along_reduced(0, vec![0, 0], vec![1, 1], Reduction::Mean).check(&shape),
        Err(ComponentRefusal::RepeatedDimension { dim: 1 })
    );
}

/// Break it: accept a norm of one dimension.
#[test]
fn a_norm_does_not_reduce_one_dimension() {
    assert_eq!(
        along_reduced(0, vec![0, 0], vec![1], Reduction::Norm).check(&shape(&[3, 2])),
        Err(ComponentRefusal::NotAVector { rank: 2 })
    );
}

/// Break it: let `Reduce` compute its mean with a loop of its own that counts the `NaN`.
#[test]
fn reducing_every_other_dimension_equals_reduce() {
    // Two paths to one arithmetic: `AlongReduced` over the only other dimension
    // of a `[1, 6]` shape, and `Reduce`.
    let shape = shape(&[1, 6]);
    let mut row = line(6);
    row[4] = f64::NAN;
    let reduced = along_reduced(0, vec![0, 0], vec![1], Reduction::Mean)
        .select(&shape, &row)
        .expect("a mean");
    let all = ComponentPick::Reduce(Reduction::Mean)
        .select(&shape, &row)
        .expect("a mean");
    assert_eq!(reduced.len(), 1);
    assert!(same(&reduced, &all));
    assert_eq!(all, vec![(0.0 + 1.0 + 2.0 + 3.0 + 5.0) / 5.0]);
}

/// Break it: rename `"mean"` to `"average"`.
#[test]
fn reduction_ids_are_stable() {
    // 01 §4.6: the ids written in a view document.
    for reduction in Reduction::ALL {
        assert_eq!(Reduction::from_id(reduction.id()), Some(reduction));
    }
    let ids: Vec<&str> = Reduction::ALL.iter().map(|r| r.id()).collect();
    assert_eq!(ids, vec!["mean", "min", "max", "norm"]);
    assert_eq!(Reduction::from_id("median"), None);
}

/// The block of a row a narrowed pick designates, cut here by decoding every
/// linear position (independent of `ravel`).
fn sub_block(row: &[f64], dims: &[usize], start: &[u64], count: &[u64]) -> Vec<f64> {
    let mut out = Vec::new();
    for (linear, value) in row.iter().enumerate() {
        let mut rest = linear;
        let mut at = vec![0usize; dims.len()];
        for k in (0..dims.len()).rev() {
            at[k] = rest % dims[k];
            rest /= dims[k];
        }
        let inside = at
            .iter()
            .enumerate()
            .all(|(k, i)| (*i as u64) >= start[k] && (*i as u64) < start[k] + count[k]);
        if inside {
            out.push(*value);
        }
    }
    out
}

/// Break it: keep `at` (instead of zeros) in the restated pick (an index outside the block).
#[test]
fn a_narrowed_pick_selects_what_the_pick_selects() {
    let full = shape(&[3, 2, 4]);
    let row = line(24);
    let picks = [
        ComponentPick::At(vec![2, 1, 3]),
        ComponentPick::Along {
            axis: 2,
            at: vec![1, 0, 0],
        },
        along_reduced(0, vec![0, 1, 0], vec![2], Reduction::Max),
    ];
    for pick in picks {
        let narrowed = pick
            .narrowed(&full)
            .expect("fits")
            .expect("a pick that reads a block");
        let block = sub_block(&row, &full.dims, &narrowed.start, &narrowed.count);
        assert_eq!(
            narrowed.pick.select(&narrowed.shape, &block).expect("on the block"),
            pick.select(&full, &row).expect("on the row"),
            "{pick:?}"
        );
    }
    assert_eq!(ComponentPick::All.narrowed(&full), Ok(None));
    assert_eq!(ComponentPick::Reduce(Reduction::Mean).narrowed(&full), Ok(None));
}
