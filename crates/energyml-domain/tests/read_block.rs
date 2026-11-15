//! Reading a block of an array, and the blocks a property of a given shape needs.
//!
//! Sources: RESQML Technical Usage Guide v2.2 §4.2.2 p. 39 ("when viewed as an
//! equivalent 1D array, the HDF5 data storage ordering and the RESQML index
//! ordering are identical"), §6.6.3 p. 65 (the order of the dimensions of a
//! property); EML TRG v2.3 `ExternalDataArrayPart` (`StartIndex` and `Count`
//! designate a window of the dataset). The datasets are numbers held in memory
//! by `slabs::Slabs`: no file is read.

use energyml_domain::Error;
use energyml_domain::property::shape::{ComponentPick, ElementAxes, ElementShape, PropertyShape, TimeAxis};
use energyml_domain::values::{self, Block, Values};
use energyml_eml_2_3 as eml23;

use crate::slabs::Slabs;

/// A part of `/rh` over the whole dataset (`[4, 3, 2]`).
fn part(start: Vec<i64>, count: Vec<i64>) -> eml23::ExternalDataArrayPart {
    eml23::ExternalDataArrayPart {
        count,
        path_in_external_file: "/rh".into(),
        start_index: start,
        uri: "t.h5".into(),
        mime_type: Some("application/x-hdf5".into()),
    }
}

fn external(part: eml23::ExternalDataArrayPart) -> eml23::FloatingPointExternalArray {
    eml23::FloatingPointExternalArray {
        count_per_value: 1,
        values: eml23::ExternalDataArray {
            external_data_array_part: vec![part],
        },
        ..Default::default()
    }
}

/// `/rh`: `[4, 3, 2]`, value = 100 n + 10 i + j.
fn rh() -> Slabs {
    let mut values = Vec::new();
    for n in 0..4 {
        for i in 0..3 {
            for j in 0..2 {
                values.push((100 * n + 10 * i + j) as f64);
            }
        }
    }
    Slabs::new().with_dataset("/rh", vec![4, 3, 2], values)
}

fn floats(values: Values) -> Vec<f64> {
    values.floating().expect("floating-point values").to_vec()
}

fn block(start: &[u64], count: &[u64]) -> Block {
    Block {
        start: start.to_vec(),
        count: count.to_vec(),
    }
}

/// Break it: pass an empty `count` to the source (it answers 24 values: the comparison of the lengths refuses).
#[test]
fn a_block_reads_only_its_values() {
    let array = external(part(vec![0, 0, 0], vec![4, 3, 2]));
    let source = rh();
    let read = values::read_block(&array, &source, "rh", &block(&[0, 0, 1], &[4, 3, 1])).expect("a block");
    let mut expected = Vec::new();
    for n in 0..4 {
        for i in 0..3 {
            expected.push((100 * n + 10 * i + 1) as f64);
        }
    }
    assert_eq!(floats(read), expected);
    assert_eq!(source.reads.get(), 1, "one read");
    assert_eq!(source.values_returned.get(), 12, "the source returned the block alone");
}

/// Break it: remove the comparison of the lengths in `read_block`.
#[test]
fn a_source_that_ignores_the_block_is_refused_by_name() {
    let array = external(part(vec![0, 0, 0], vec![4, 3, 2]));
    let source = rh().whole_only();
    let refused = values::read_block(&array, &source, "rh", &block(&[0, 0, 1], &[4, 3, 1]))
        .expect_err("a source that reads whole is refused");
    match refused {
        Error::Malformed { reason, .. } => assert!(reason.contains("does not read by block"), "{reason}"),
        other => panic!("expected a malformed refusal, got {other:?}"),
    }
}

/// Break it: ignore `loc.start` in `read_block`.
#[test]
fn a_part_window_offsets_the_block() {
    // EML TRG 2.3 `ExternalDataArrayPart`: the part is the window `[1..3, .., ..]` of the dataset.
    let array = external(part(vec![1, 0, 0], vec![2, 3, 2]));
    let source = rh();
    let read = values::read_block(&array, &source, "rh", &block(&[0, 0, 0], &[1, 3, 2])).expect("a block");
    // The first node of the window is the node 1 of the dataset.
    let mut expected = Vec::new();
    for i in 0..3 {
        for j in 0..2 {
            expected.push((100 + 10 * i + j) as f64);
        }
    }
    assert_eq!(floats(read), expected);
    assert_eq!(values::stored_dims(&array, &source, "rh").expect("dims"), vec![2, 3, 2]);
}

/// Break it: remove the bounds check.
#[test]
fn a_block_past_the_array_is_refused() {
    let array = external(part(vec![0, 0, 0], vec![4, 3, 2]));
    let source = rh();
    let refused = values::read_block(&array, &source, "rh", &block(&[3, 0, 0], &[2, 3, 2]))
        .expect_err("past the end");
    match refused {
        Error::Malformed { reason, .. } => assert!(reason.contains("dimension 0"), "{reason}"),
        other => panic!("expected a malformed refusal, got {other:?}"),
    }
}

/// Break it: see a lattice as `[12]`.
#[test]
fn a_computed_array_is_cut_like_a_stored_one() {
    // Offsets of 3 and 2 steps: `[4, 3]`, value = 10 a + b.
    let lattice = eml23::FloatingPointLatticeArray {
        start_value: 0.0,
        offset: vec![
            eml23::FloatingPointConstantArray {
                value: 10.0,
                count: 3,
                ..Default::default()
            },
            eml23::FloatingPointConstantArray {
                value: 1.0,
                count: 2,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let source = Slabs::new();
    assert_eq!(values::stored_dims(&lattice, &source, "lattice").expect("dims"), vec![4, 3]);
    let whole = floats(values::read(&lattice, &source, "lattice").expect("whole"));
    let cut = floats(values::read_block(&lattice, &source, "lattice", &block(&[1, 0], &[2, 3])).expect("block"));
    assert_eq!(cut, whole[3..9].to_vec());
}

fn property_shape(elements: ElementAxes, components: &[u64]) -> PropertyShape {
    PropertyShape {
        realizations: None,
        time: None,
        elements,
        components: ElementShape::of(components, &[]).expect("a valid shape"),
    }
}

/// Break it: forget to multiply by the values per element.
#[test]
fn an_element_block_of_a_flat_store() {
    let shape = property_shape(ElementAxes::Ordered(vec![4]), &[3, 2]);
    let layout = shape.layout(&[24]).expect("a flat store");
    assert_eq!(
        values::element_block(&shape, &layout, 1, 2).expect("a block"),
        block(&[6], &[12])
    );
}

/// Break it: forget to multiply by the values per element.
#[test]
fn an_element_block_of_an_nd_store() {
    let shape = property_shape(ElementAxes::Ordered(vec![4]), &[3, 2]);
    let layout = shape.layout(&[4, 3, 2]).expect("an ND store");
    assert_eq!(
        values::element_block(&shape, &layout, 1, 2).expect("a block"),
        block(&[1, 0, 0], &[2, 3, 2])
    );
}

/// Break it: accept a run of elements over several stored dimensions.
#[test]
fn a_run_of_elements_over_several_stored_dimensions_is_refused() {
    // Cells `[2, 3]` stored as `[2, 3]`: a run of cells is not a rectangle.
    let shape = property_shape(ElementAxes::Ordered(vec![2, 3]), &[]);
    let layout = shape.layout(&[2, 3]).expect("as declared");
    assert!(matches!(
        values::element_block(&shape, &layout, 1, 2),
        Err(Error::Unsupported { .. })
    ));
}

// ---------------------------------------------------------------------------
// slab_block
// ---------------------------------------------------------------------------

fn three_steps(components: &[u64]) -> PropertyShape {
    PropertyShape {
        realizations: None,
        time: Some(TimeAxis::of(3, false)),
        elements: ElementAxes::Ordered(vec![4]),
        components: ElementShape::of(components, &[]).expect("a valid shape"),
    }
}

/// Break it: forget the `fixed` term in the group holding the slabs (the last two cases read step 0).
#[test]
fn a_slab_of_one_time_step() {
    // Usage Guide 2.2 §4.2.2 p. 39 and §6.6.3 p. 65: `[time][elements][components]`.
    let shape = three_steps(&[2]);
    let at = |stored: &[u64]| {
        let layout = shape.layout(stored).expect("a layout");
        values::slab_block(&shape, &layout, &[1], 1, 2, None).expect("a slab").block
    };
    assert_eq!(at(&[3, 4, 2]), block(&[1, 1, 0], &[1, 2, 2]));
    assert_eq!(at(&[3, 8]), block(&[1, 2], &[1, 4]));
    assert_eq!(at(&[24]), block(&[10], &[4]));
}

/// Break it: narrow a grouped dimension as well.
#[test]
fn components_are_narrowed_only_when_stored_apart() {
    let shape = property_shape(ElementAxes::Ordered(vec![4]), &[3, 2]);
    let components = shape_of_components();
    let pick = ComponentPick::At(vec![2, 1]);
    let narrowed = pick.narrowed(&components).expect("fits").expect("a block");

    let apart = shape.layout(&[4, 3, 2]).expect("apart");
    let slab = values::slab_block(&shape, &apart, &[], 0, 4, Some(&narrowed)).expect("a slab");
    assert_eq!(slab.block, block(&[0, 2, 1], &[4, 1, 1]));
    assert!(slab.narrowed);

    let grouped = shape.layout(&[4, 6]).expect("grouped");
    let slab = values::slab_block(&shape, &grouped, &[], 0, 4, Some(&narrowed)).expect("a slab");
    assert_eq!(slab.block, block(&[0, 0], &[4, 6]));
    assert!(!slab.narrowed);
}

fn shape_of_components() -> ElementShape {
    ElementShape::of(&[3, 2], &[]).expect("a valid shape")
}

/// Break it: `<=` instead of `<`.
#[test]
fn a_fixed_index_past_its_dimension_is_refused() {
    let shape = three_steps(&[]);
    let layout = shape.layout(&[3, 4]).expect("a layout");
    let refused = values::slab_block(&shape, &layout, &[3], 0, 1, None).expect_err("past the 3 steps");
    match refused {
        Error::Malformed { reason, .. } => assert!(reason.contains("dimension 0"), "{reason}"),
        other => panic!("expected a malformed refusal, got {other:?}"),
    }
}

/// A slab is read with `read_block`: the values of step 1, elements 1 and 2.
/// Break it: pass the pick's `At` unrestated.
#[test]
fn a_slab_read_by_block_equals_the_cut_of_the_whole() {
    let shape = three_steps(&[2]);
    let layout = shape.layout(&[3, 4, 2]).expect("a layout");
    let whole: Vec<f64> = (0..24).map(f64::from).collect();
    let source = Slabs::new().with_dataset("/p", vec![3, 4, 2], whole.clone());
    let array = eml23::FloatingPointExternalArray {
        count_per_value: 1,
        values: eml23::ExternalDataArray {
            external_data_array_part: vec![eml23::ExternalDataArrayPart {
                count: vec![3, 4, 2],
                path_in_external_file: "/p".into(),
                start_index: vec![0, 0, 0],
                uri: "t.h5".into(),
                mime_type: None,
            }],
        },
        ..Default::default()
    };
    let slab = values::slab_block(&shape, &layout, &[1], 1, 2, None).expect("a slab");
    let read = floats(values::read_block(&array, &source, "p", &slab.block).expect("a block"));
    // Step 1 starts at 8; its elements 1 and 2 are the values 10..14.
    assert_eq!(read, whole[10..14].to_vec());
    assert_eq!(source.values_returned.get(), 4, "only the slab was read");
}
