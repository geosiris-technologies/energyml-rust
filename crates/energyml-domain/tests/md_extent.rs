//! From which MD to which, `energyml_domain::md_extent` (WP06 §4.13).
//!
//! Every object is built in memory after the specification, never after a package:
//!
//! - EML 2.3 `MdInterval` (`MdMin`, `MdMax`, `Uom`, `Datum`): *"The datum the MD interval is
//!   referenced to"*; WITSML 2.1 `Log.PrimaryIndexInterval` is an `AbstractInterval`
//!   (`MdInterval`, `DateTimeInterval`, ...);
//! - Energistics common TRG 2.3 §6.48 p. 134: *"All coordinates with the same datum (and
//!   same UOM) can be considered to be in the same coordinate reference system (CRS) and are
//!   thus directly comparable."* - the rule `strata_by_md::comparable` writes, and which a
//!   coverage never rewrites;
//! - RESQML TRG 2.2 §12.13 p. 310: a frame's `NodeMd` are consistent with its trajectory.
//!
//! The last line of each comment says how to make the test fail.

use std::borrow::Cow;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::md_extent::{
    Alignment, CoverageItem, CoverageNote, Extent, ExtentAxis, ExtentBasis, ExtentReader, ExtentReaders,
    ExtentRefusal, Inherit, coverage, of_object, of_rows,
};
use energyml_domain::source::{ArraySource, NoObjects, Prefetched};
use energyml_domain::table::{
    ColumnCells, Convention, IntervalColumns, LinkDeclaration, LinkTarget, Lookup, TableRefusal, TableSet,
};
use energyml_domain::values::Values;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::ArrayLocation;
use energyml_witsml_2_1 as witsml21;

const TRAJECTORY_A: &str = "a0000000-0000-4000-8000-00000000000a";
const TRAJECTORY_B: &str = "a0000000-0000-4000-8000-00000000000b";
const LOG: &str = "a0000000-0000-4000-8000-0000000000c1";
const FRAME: &str = "a0000000-0000-4000-8000-0000000000f1";
const DATUM_1: &str = "d0000000-0000-4000-8000-000000000001";
const DATUM_2: &str = "d0000000-0000-4000-8000-000000000002";

// -- building blocks ---------------------------------------------------------

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        originator: "test".to_string(),
        creation: "2026-11-14T00:00:00Z".to_string(),
        format: "test".to_string(),
        ..Default::default()
    }
}

fn dor(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: "a target".to_string(),
        ..Default::default()
    }
}

fn md_interval(min: f64, max: f64, uom: eml23::LengthUomExt, datum: &str) -> eml23::MdInterval {
    eml23::MdInterval {
        md_min: min,
        md_max: max,
        uom,
        datum: Some(dor(datum, "eml23.ReferencePointInACrs")),
        ..Default::default()
    }
}

fn trajectory(
    uuid: &str,
    datum: &str,
    uom: eml23::LengthUomExt,
    min: f64,
    max: f64,
) -> resqml22::WellboreTrajectoryRepresentation {
    resqml22::WellboreTrajectoryRepresentation {
        uuid: uuid.to_string(),
        citation: citation("trajectory"),
        md_interval: md_interval(min, max, uom, datum),
        md_domain: Some(resqml22::MdDomain::Driller),
        ..Default::default()
    }
}

fn extent_of(object: &dyn Reflect) -> Extent {
    of_object(object, &NoObjects, &ExtentReaders::builtin()).expect("an extent")
}

fn item(key: &str, extent: Extent) -> CoverageItem {
    CoverageItem {
        key: key.to_string(),
        extent,
    }
}

/// A row's extent as a table column gives it: no reference.
fn row_extent(from: f64, to: f64) -> Extent {
    Extent {
        from,
        to,
        uom: Some("m".to_string()),
        reference: None,
        axis: ExtentAxis::Md,
        basis: ExtentBasis::Declared,
        reader: "table-rows",
    }
}

fn nowhere(_: &dyn Reflect) -> Vec<ArrayLocation> {
    Vec::new()
}

fn floats(values: Vec<f64>) -> Result<ColumnCells<'static>, TableRefusal> {
    Ok(ColumnCells {
        values: Cow::Owned(Values::Floating(values)),
        per_row: 1,
        null: None,
    })
}

fn texts(values: &[&str]) -> Result<ColumnCells<'static>, TableRefusal> {
    Ok(ColumnCells {
        values: Cow::Owned(Values::Text(values.iter().map(|text| text.to_string()).collect())),
        per_row: 1,
        null: None,
    })
}

// -- the extent of one object ------------------------------------------------

#[test]
fn an_md_interval_field_gives_a_declared_extent() {
    let object = witsml21::Trajectory {
        uuid: TRAJECTORY_A.to_string(),
        citation: citation("a trajectory"),
        md_interval: Some(md_interval(0.0, 2900.0, eml23::LengthUomExt::M, DATUM_1)),
        ..Default::default()
    };
    let found = extent_of(&object);
    assert_eq!((found.from, found.to), (0.0, 2900.0));
    assert_eq!(found.basis, ExtentBasis::Declared);
    assert_eq!(found.reader, "declared-md-interval");
    let reference = found.reference.expect("a reference");
    assert_eq!(reference.datum.as_deref(), Some(DATUM_1));
    assert_eq!(
        reference.trajectory, TRAJECTORY_A,
        "no trajectory counts these MDs: the object's own"
    );
    assert_eq!(reference.uom.as_deref(), Some("m"));
    // Break it: write `trajectory: String::new()`, and two objects look like one trajectory.
}

#[test]
fn a_resqml_trajectory_gives_its_interval_and_how_it_is_counted() {
    let found = extent_of(&trajectory(
        TRAJECTORY_A,
        DATUM_1,
        eml23::LengthUomExt::M,
        5.0,
        40.0,
    ));
    assert_eq!((found.from, found.to), (5.0, 40.0));
    assert_eq!(found.reader, "resqml-md-reference");
    let reference = found.reference.expect("a reference");
    assert_eq!(reference.trajectory, TRAJECTORY_A);
    assert_eq!(reference.datum.as_deref(), Some(DATUM_1));
    assert_eq!(reference.domain.as_deref(), Some("driller"));
    // Break it: read the datum from the wrong field, and `datum` is `None`.
}

#[test]
fn a_witsml21_log_reads_its_primary_index_interval() {
    let log = witsml21::Log {
        uuid: LOG.to_string(),
        citation: citation("a log"),
        primary_index_interval: Some(witsml21::AbstractInterval::MdInterval(md_interval(
            2900.0,
            3200.0,
            eml23::LengthUomExt::M,
            DATUM_1,
        ))),
        ..Default::default()
    };
    let found = extent_of(&log);
    assert_eq!((found.from, found.to), (2900.0, 3200.0));
    assert_eq!(found.reader, "primary-index-interval");
    // Break it: read `StartIndex` instead, or look at the wrong field name, and this test is red.
}

#[test]
fn a_time_index_is_out_of_the_md_axis() {
    // WITSML TRG v2.1 §7.7 p. 151: a log is indexed by depth or by time.
    let log = witsml21::Log {
        uuid: LOG.to_string(),
        citation: citation("a time log"),
        primary_index_interval: Some(witsml21::AbstractInterval::DateTimeInterval(
            eml23::DateTimeInterval {
                start_time: "2026-01-01T00:00:00Z".to_string(),
                end_time: "2026-01-02T00:00:00Z".to_string(),
                ..Default::default()
            },
        )),
        ..Default::default()
    };
    let refused = of_object(&log, &NoObjects, &ExtentReaders::builtin());
    assert!(
        matches!(
            refused,
            Err(ExtentRefusal::NotDepth {
                axis: ExtentAxis::Time
            })
        ),
        "got {refused:?}"
    );
    // Break it: answer an extent 0..0, and the time log is drawn on the depth axis.
}

#[test]
fn an_object_that_states_no_interval_is_refused_by_name() {
    let table = eml23::ColumnBasedTable::default();
    let refused = of_object(&table, &NoObjects, &ExtentReaders::builtin());
    assert!(
        matches!(refused, Err(ExtentRefusal::NoExtent { .. })),
        "got {refused:?}"
    );
    // Break it: return an empty extent for an object no reader knows.
}

#[test]
fn a_frame_extent_is_the_range_of_its_node_mds() {
    // RESQML TRG 2.2 §12.13 p. 310: the NodeMd are the trajectory's MDs.
    let frame = resqml22::WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
        citation: citation("a frame"),
        node_count: 3,
        node_md: Some(eml23::AbstractFloatingPointArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values: vec![30.0, 5.0, 12.0],
                ..Default::default()
            },
        )),
        trajectory: dor(TRAJECTORY_A, "resqml22.WellboreTrajectoryRepresentation"),
        ..Default::default()
    };
    let mut cache = Prefetched::new();
    cache.insert(
        Box::new(trajectory(
            TRAJECTORY_A,
            DATUM_1,
            eml23::LengthUomExt::M,
            0.0,
            40.0,
        )) as Box<dyn DynObject>,
        &nowhere,
    );
    let found = of_object(&frame, &cache, &ExtentReaders::builtin()).expect("an extent");
    assert_eq!(
        (found.from, found.to),
        (5.0, 30.0),
        "least and greatest, not first and last"
    );
    assert_eq!(found.basis, ExtentBasis::Measured);
    assert_eq!(found.reader, "node-md");
    assert_eq!(found.reference.expect("a reference").trajectory, TRAJECTORY_A);
    // Break it: take the first and last node, and the range is 30..12.
}

#[test]
fn a_callers_extent_reader_is_tried_first() {
    struct Fixed;
    impl ExtentReader for Fixed {
        fn id(&self) -> &'static str {
            "test"
        }
        fn read(
            &self,
            _object: &dyn Reflect,
            _source: &dyn ArraySource,
        ) -> Option<Result<Extent, ExtentRefusal>> {
            Some(Ok(Extent {
                reader: "test",
                ..row_extent(1.0, 2.0)
            }))
        }
    }
    let mut readers = ExtentReaders::builtin();
    readers.push(Box::new(Fixed));
    assert_eq!(readers.ids()[0], "test");
    let object = trajectory(TRAJECTORY_A, DATUM_1, eml23::LengthUomExt::M, 0.0, 10.0);
    let found = of_object(&object, &NoObjects, &readers).expect("an extent");
    assert_eq!(found.reader, "test");
    assert_eq!(ExtentReaders::none().ids().len(), 0);
    // Break it: push at the end of the list, and the built-in reader answers first.
}

// -- the rows of a table -----------------------------------------------------

#[test]
fn a_row_takes_its_own_md_columns_and_a_half_pair_is_incomplete() {
    let cells = vec![
        floats(vec![10.0, f64::NAN, 5.0]),
        floats(vec![20.0, 8.0, f64::NAN]),
    ];
    let rows = of_rows(
        &cells,
        &IntervalColumns::Pair { top: 0, base: 1 },
        Some("m"),
        None,
    );
    assert_eq!(rows.len(), 3);
    let own = rows[0].extent.as_ref().expect("a whole pair");
    assert_eq!((own.from, own.to), (10.0, 20.0));
    assert_eq!(
        own.reference, None,
        "a table column says nothing of how its MDs are counted"
    );
    assert_eq!(own.uom.as_deref(), Some("m"));
    assert!(
        rows[1].incomplete && rows[1].extent.is_none(),
        "a missing top: no extent, and said"
    );
    assert!(
        rows[2].incomplete && rows[2].extent.is_none(),
        "a missing base: no extent, and said"
    );

    let point = of_rows(&cells, &IntervalColumns::Point { md: 0 }, None, None);
    let one = point[0].extent.as_ref().expect("a point");
    assert_eq!((one.from, one.to), (10.0, 10.0));

    let nothing = of_rows(&cells, &IntervalColumns::Ambiguous(vec![0, 1, 2]), None, None);
    assert!(
        nothing.iter().all(|row| row.extent.is_none() && !row.incomplete),
        "three columns choose nothing"
    );
    // Break it: make a half pair `(x, x)`, and a missing base is drawn as a point.
}

#[test]
fn an_inherited_extent_says_through_which_column() {
    // A row with no MD whose `index_uid` names a row of 2900..3200 in the indexes table.
    let target_uri = "eml:///eml23.ColumnBasedTable(7179e925-fba7-50be-9032-5462a75484da)";
    let key = |title: &str, values: &[&str]| eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
            values: values.iter().map(|text| text.to_string()).collect(),
            count_per_value: 1,
            ..Default::default()
        })),
        ..Default::default()
    };
    let table = eml23::ColumnBasedTable {
        uuid: "7179e925-fba7-50be-9032-5462a75484da".to_string(),
        citation: citation("indexes"),
        key_column: vec![key("index_uid", &["i1", "i2"])],
        column: vec![key("label", &["first", "second"])],
        ..Default::default()
    };
    let mut targets = TableSet::new();
    targets.insert(
        target_uri,
        Some("indexes".to_string()),
        Lookup::read(&table, &NoObjects).expect("a table that reads"),
    );

    let link = LinkDeclaration {
        column: 2,
        target: LinkTarget::Key {
            table_uri: target_uri.to_string(),
        },
        convention: Convention::KindAndFacet,
        warnings: Vec::new(),
        key_named: None,
    };
    let of_target = |_uri: &str, row: usize| -> Option<Extent> {
        (row == 1).then(|| Extent {
            reader: "node-md",
            ..row_extent(2900.0, 3200.0)
        })
    };
    let inherit = Inherit {
        link: &link,
        own_uri: "eml:///eml23.ColumnBasedTable(00000000-0000-4000-8000-0000000000cc)",
        targets: &targets,
        extent_of: &of_target,
    };
    let cells = vec![floats(vec![f64::NAN]), floats(vec![f64::NAN]), texts(&["i2"])];
    let rows = of_rows(
        &cells,
        &IntervalColumns::Pair { top: 0, base: 1 },
        Some("m"),
        Some(&inherit),
    );
    let got = rows[0].extent.as_ref().expect("the extent of the row it names");
    assert_eq!((got.from, got.to), (2900.0, 3200.0));
    assert_eq!(got.basis, ExtentBasis::ThroughLink { column: 2 });
    assert!(!rows[0].incomplete);

    // The same row without a link to follow has no extent.
    let alone = of_rows(
        &cells,
        &IntervalColumns::Pair { top: 0, base: 1 },
        Some("m"),
        None,
    );
    assert!(alone[0].extent.is_none());
    // Break it: give `basis: Declared` to an inherited extent, and the view cannot say it was borrowed.
}

// -- the coverage ------------------------------------------------------------

#[test]
fn a_reversed_interval_is_kept_and_said() {
    let found = extent_of(&trajectory(
        TRAJECTORY_A,
        DATUM_1,
        eml23::LengthUomExt::M,
        10.0,
        5.0,
    ));
    assert_eq!(
        (found.from, found.to),
        (10.0, 5.0),
        "kept as written, never swapped"
    );
    assert!(found.is_reversed());
    let drawn = coverage(vec![item("a", found)], &[]);
    assert!(drawn.notes.contains(&CoverageNote::Reversed {
        item: "a".to_string()
    }));
    assert_eq!(drawn.groups[0].bars[0].from, 10.0, "drawn as written");
    // Break it: swap the bounds on reading, and the note and the order are both lost.
}

#[test]
fn two_datums_are_two_groups() {
    let first = extent_of(&trajectory(
        TRAJECTORY_A,
        DATUM_1,
        eml23::LengthUomExt::M,
        0.0,
        100.0,
    ));
    let second = extent_of(&trajectory(
        TRAJECTORY_B,
        DATUM_2,
        eml23::LengthUomExt::M,
        0.0,
        100.0,
    ));
    let drawn = coverage(vec![item("a", first), item("b", second)], &[]);
    assert_eq!(
        drawn.groups.len(),
        2,
        "an MD is never moved from one datum to the other"
    );
    let why = drawn
        .notes
        .iter()
        .find_map(|note| match note {
            CoverageNote::Incomparable { item, why, .. } if item == "b" => Some(why.as_str()),
            _ => None,
        })
        .expect("a note on the second item");
    assert!(
        why.contains("§6.48"),
        "the reason cites common TRG 2.3 §6.48: {why}"
    );
    // Break it: put everything in one group, and there is neither a second group nor a note.
}

#[test]
fn feet_and_metres_share_an_axis_converted() {
    // Same datum, same MD domain, two units that convert (a distinct trajectory each: one
    // trajectory counts in one unit, and `comparable` takes it at its word).
    let metres = extent_of(&trajectory(
        TRAJECTORY_A,
        DATUM_1,
        eml23::LengthUomExt::M,
        0.0,
        100.0,
    ));
    let feet = extent_of(&trajectory(
        TRAJECTORY_B,
        DATUM_1,
        eml23::LengthUomExt::Ft,
        0.0,
        100.0,
    ));
    let drawn = coverage(vec![item("m", metres), item("ft", feet)], &[]);
    assert_eq!(drawn.groups.len(), 1, "one datum, one domain: one axis");
    let bar = &drawn.groups[0].bars[1];
    assert!((bar.to - 30.48).abs() < 1e-9, "100 ft is 30.48 m, got {}", bar.to);
    assert_eq!(drawn.groups[0].uom.as_deref(), Some("m"));
    assert!(drawn.notes.is_empty());
    // Break it: keep the value as written, and the bar is 100 on a metre axis.
}

#[test]
fn an_alignment_merges_and_is_written() {
    // 07 §4.5: the person says "count this row's MDs as the trajectory's are counted".
    let trajectory = extent_of(&trajectory(
        TRAJECTORY_A,
        DATUM_1,
        eml23::LengthUomExt::M,
        0.0,
        100.0,
    ));
    let row = row_extent(10.0, 20.0);
    let alignment = Alignment {
        item: "row".to_string(),
        onto: "traj".to_string(),
    };
    // The row is first: the order of the items does not decide which group it lands in.
    let drawn = coverage(
        vec![item("row", row), item("traj", trajectory)],
        std::slice::from_ref(&alignment),
    );
    assert_eq!(drawn.groups.len(), 1);
    assert_eq!(drawn.groups[0].bars.len(), 2);
    assert!(
        drawn.groups[0]
            .bars
            .iter()
            .find(|bar| bar.key == "row")
            .expect("the row")
            .aligned
    );
    assert!(
        !drawn.groups[0]
            .bars
            .iter()
            .find(|bar| bar.key == "traj")
            .expect("the trajectory")
            .aligned
    );
    assert_eq!(drawn.applied, vec![alignment]);
    // Break it: ignore the alignment, and the row goes to a band of its own.
}

#[test]
fn an_alignment_onto_nothing_is_noted_and_ignored() {
    let alignment = Alignment {
        item: "row".to_string(),
        onto: "missing".to_string(),
    };
    let drawn = coverage(
        vec![item("row", row_extent(1.0, 2.0))],
        std::slice::from_ref(&alignment),
    );
    assert!(
        drawn
            .notes
            .contains(&CoverageNote::UnknownAlignment { alignment })
    );
    assert!(drawn.applied.is_empty());
    assert_eq!(drawn.groups[0].reference, None);
    // Break it: apply it anyway, and `applied` lists an alignment that moved nothing.
}

#[test]
fn a_table_row_without_reference_goes_to_the_unstated_band() {
    let trajectory = extent_of(&trajectory(
        TRAJECTORY_A,
        DATUM_1,
        eml23::LengthUomExt::M,
        0.0,
        100.0,
    ));
    let drawn = coverage(
        vec![item("traj", trajectory), item("row", row_extent(10.0, 20.0))],
        &[],
    );
    assert_eq!(drawn.groups.len(), 2);
    assert!(drawn.groups[0].reference.is_some());
    assert_eq!(drawn.groups[1].reference, None, "the band of unstated references");
    assert_eq!(drawn.groups[1].bars[0].key, "row");
    // Break it: attach the row to the first group, and the picture claims an alignment nobody made.
}

#[test]
fn an_extent_off_the_md_axis_is_counted_apart() {
    let time = Extent {
        axis: ExtentAxis::Time,
        ..row_extent(0.0, 1.0)
    };
    let drawn = coverage(vec![item("log", time)], &[]);
    assert!(drawn.groups.is_empty(), "no bar on the MD axis");
    assert!(drawn.notes.contains(&CoverageNote::OffAxis {
        item: "log".to_string(),
        axis: ExtentAxis::Time
    }));
    // Break it: draw it on the depth axis, and a time series sits at depth 0..1.
}
