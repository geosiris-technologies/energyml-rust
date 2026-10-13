//! A borehole read as a table, and several in one file.
//!
//! What can be **wrong** here is the merging of several wells — a column that
//! slips by one rank, two different measurements under one header, a label that
//! splits its line in two — and none of that needs a file. The corpus serves at
//! the end, to say that the reading holds on real objects.

#[cfg(feature = "hdf5")]
use std::path::PathBuf;

use energyml_domain::wellbore_table::{self, Column, Conflict, From, WellTable};

/// A hand-written column.
fn column(title: &str, uom: Option<&str>, kind: Option<&str>, values: &[Option<&str>]) -> Column {
    Column {
        title: title.to_string(),
        uom: uom.map(str::to_string),
        kind: kind.map(str::to_string),
        kind_title: None,
        from: From::Property(format!("uuid-{title}")),
        values: values.iter().map(|one| one.map(str::to_string)).collect(),
    }
}

/// A hand-written borehole: its depth, then its columns.
fn well(name: &str, mds: &[&str], columns: Vec<Column>) -> WellTable {
    let mut all = vec![Column {
        title: "MD".to_string(),
        uom: None,
        kind: None,
        kind_title: None,
        from: From::MeasuredDepth,
        values: mds.iter().map(|one| Some(one.to_string())).collect(),
    }];
    all.extend(columns);
    WellTable {
        well: name.to_string(),
        uuid: format!("uuid-{name}"),
        columns: all,
        rows: mds.len(),
        notes: Vec::new(),
    }
}

#[test]
fn a_well_that_lacks_a_column_gets_empty_cells_and_never_a_shifted_row() {
    // **The point that carries the merge.** F2 has no `PHI`; if its absence
    // shifted the row, F2's `STRA` would land under the `PHI` header — a
    // perfectly well-formed CSV half of whose columns lie.
    let one = well("F1", &["10", "20"], vec![
        column("PHI", Some("v/v"), Some("k-phi"), &[Some("0.2"), Some("0.3")]),
        column("STRA", None, Some("k-stra"), &[Some("argile"), Some("sable")]),
    ]);
    let two = well("F2", &["5"], vec![column("STRA", None, Some("k-stra"), &[Some("marne")])]);

    let merged = wellbore_table::merge(&[one, two]);
    assert_eq!(merged.headers, ["well", "MD", "PHI", "STRA"], "the union, in order of appearance");
    assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);
    assert_eq!(
        merged.rows,
        vec![
            vec![Some("F1".into()), Some("10".into()), Some("0.2".into()), Some("argile".into())],
            vec![Some("F1".into()), Some("20".into()), Some("0.3".into()), Some("sable".into())],
            // F2 has no `PHI`: the cell is **empty**, and `marne` stays under
            // `STRA`.
            vec![Some("F2".into()), Some("5".into()), None, Some("marne".into())],
        ]
    );
}

#[test]
fn two_wells_whose_category_tables_differ_still_merge() {
    // **That is what resolution into labels buys.** F1 codes `argile` as 0 and
    // F2 codes it as 1; once resolved, the two columns say the same thing and
    // stick together. Exporting the raw codes would have made one figure two
    // materials.
    let one = well("F1", &["10"], vec![column("STRA", None, Some("k"), &[Some("argile")])]);
    let two = well("F2", &["10"], vec![column("STRA", None, Some("k"), &[Some("argile")])]);
    let merged = wellbore_table::merge(&[one, two]);
    assert!(merged.conflicts.is_empty());
    assert_eq!(merged.rows.len(), 2);
    // `well, MD, STRA`: the lithology is the third column.
    assert_eq!(merged.rows[0][2], merged.rows[1][2]);
}

#[test]
fn two_measurements_under_one_heading_are_named_and_not_merged() {
    // Millidarcies from one borehole and square metres from another in one
    // column give a file that nothing downstream can detect. It is said, and the
    // caller decides — split the files, or write anyway.
    let one = well("F1", &["10"], vec![column("PERM", Some("mD"), Some("k-perm"), &[Some("1")])]);
    let two = well("F2", &["10"], vec![column("PERM", Some("m2"), Some("k-perm"), &[Some("2")])]);
    let merged = wellbore_table::merge(&[one, two]);
    assert_eq!(
        merged.conflicts,
        [Conflict::Uom { column: "PERM".into(), first: Some("mD".into()), other: Some("m2".into()) }]
    );
    assert!(merged.conflicts[0].says().contains("mD") && merged.conflicts[0].says().contains("m2"));

    // And two kinds under one title are the other half: same name, two
    // measurements.
    let a = well("F1", &["10"], vec![column("X", None, Some("k-a"), &[Some("1")])]);
    let b = well("F2", &["10"], vec![column("X", None, Some("k-b"), &[Some("2")])]);
    let clash = wellbore_table::merge(&[a, b]);
    assert_eq!(clash.conflicts.len(), 1);
    assert!(matches!(clash.conflicts[0], Conflict::Kind { .. }), "{:?}", clash.conflicts);
}

#[test]
fn a_label_that_carries_the_separator_is_quoted() {
    // RFC 4180, and it is not theoretical: a lithology is written
    // "argile, sableuse". Without quoting, the line splits in two — and the
    // file remains perfectly well formed.
    let one = well("F1", &["10", "20"], vec![column(
        "STRA",
        None,
        None,
        &[Some("argile, sableuse"), Some("marne \"dure\"")],
    )]);
    let csv = wellbore_table::to_csv(&wellbore_table::merge(&[one]), ',');
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "well,MD,STRA");
    assert_eq!(lines[1], "F1,10,\"argile, sableuse\"");
    // A quote is doubled — that is the format's rule, not an escape of our own.
    assert_eq!(lines[2], "F1,20,\"marne \"\"dure\"\"\"");

    // And with a semicolon, the comma no longer needs quoting: what gets quoted
    // is what **separates in this file**.
    let two = well("F1", &["10"], vec![column("STRA", None, None, &[Some("argile, sableuse")])]);
    let other = wellbore_table::to_csv(&wellbore_table::merge(&[two]), ';');
    assert_eq!(other.lines().nth(1).unwrap(), "F1;10;argile, sableuse");
}

// -- on real objects --------------------------------------------------------

#[cfg(feature = "hdf5")]
#[test]
fn every_wellbore_frame_of_the_corpus_tabulates_one_row_per_node_or_pass() {
    tabulate(crate::fixtures::public_corpus());
}

/// The same check on the real files of the private corpus (PaleoGTM: 443
/// frames). Real-file campaign, run privately.
#[cfg(all(feature = "hdf5", feature = "private-corpus"))]
#[test]
fn every_wellbore_frame_of_the_private_corpus_tabulates_one_row_per_node_or_pass() {
    tabulate(crate::fixtures::private_corpus());
}

#[cfg(feature = "hdf5")]
fn tabulate(paths: Vec<PathBuf>) {
    // **The shape comes from the spec, the values from the file.** What is
    // required is structural: the depth is the first column, and each column has
    // exactly as many values as the table has rows — one per node, or one per
    // pass. A shorter column would be the silent shift this whole module exists
    // to prevent.
    let mut seen = 0usize;
    for path in paths {
        let registry = std::sync::Arc::new(energyml_schemas::all());
        let Ok(store) = energyml_storage::EpcStore::open(&path, registry) else { continue };
        let summaries = store.resources();
        // Every property parsed **once per package**, then filtered per frame.
        // Parsed per frame, `h+g_PaleoGTM` (443 frames, ~960 properties) cost
        // ~424 000 XML parses and ran for over five minutes in the `hdf5` CI step
        // (29/09) — for the same answer.
        let properties: Vec<_> = summaries
            .iter()
            .filter(|other| other.qualified_type.contains("Property"))
            .filter_map(|other| store.get_object(&other.uri).ok())
            .collect();
        for summary in &summaries {
            if !summary.qualified_type.contains("WellboreFrameRepresentation") {
                continue;
            }
            let Ok(frame) = store.get_object(&summary.uri) else { continue };
            let borrowed: Vec<&dyn energyml_core::value::Reflect> = properties
                .iter()
                .map(|one| one.as_reflect())
                .filter(|object| energyml_domain::wellbore_table::hangs_on(*object, &summary.uuid))
                .collect();
            let Ok(table) = wellbore_table::of_frame(frame.as_reflect(), &borrowed, &store) else {
                continue;
            };
            seen += 1;
            // Two shapes, with the depth first in both. One row per node: `MD`.
            // One row per pass, as soon as the frame carries a property indexed
            // on its intervals: the top then the base — and a pass ends where the
            // next one begins, since both come from the same nodes
            // (`IndexableElement` `intervals`: n − 1 values for n nodes). Read the
            // shape by `from` and not by the title: that is what this check had
            // missed.
            match table.columns[0].from {
                wellbore_table::From::MeasuredDepth => {}
                wellbore_table::From::TopDepth => {
                    let (top, base) = (&table.columns[0], &table.columns[1]);
                    assert_eq!(
                        base.from,
                        wellbore_table::From::MeasuredDepth,
                        "{}: a table of passes carries the top then the base",
                        table.well
                    );
                    for at in 1..table.rows {
                        assert_eq!(
                            top.values[at], base.values[at - 1],
                            "{}: pass {at} does not begin where the previous one ends",
                            table.well
                        );
                    }
                }
                ref other => panic!("{}: the depth is the axis, so the first column — not {other:?}", table.well),
            }
            for column in &table.columns {
                assert_eq!(
                    column.values.len(),
                    table.rows,
                    "{}: `{}` has {} value(s) for {} node(s)",
                    table.well,
                    column.title,
                    column.values.len(),
                    table.rows
                );
            }
        }
    }
    if seen == 0 {
        eprintln!("corpus without a wellbore frame: check skipped");
    }
}
