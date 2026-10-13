//! The wellbores of a package in **one** CSV: everything the package says
//! about them.
//!
//! The export covers the `WellboreFrame`s of a storage (EPC or ETP), with the
//! wellbore name, its dates, the aliases of its feature, the values of its
//! properties - the labels of the categorical tables (CBT) for discrete ones -
//! and any other useful information, the headers taken from the property
//! kinds. The aim is to produce the shape of the borehole CSV that the import
//! starts from: what comes out of here must be able to go back in.
//!
//! # What is where
//!
//! - **what a frame carries** - the MD of each node and its properties, codes
//!   resolved to labels - is [`energyml_domain::wellbore_table`], which the
//!   well view reads too;
//! - **where it is** - the head and the coordinates of each node - is the
//!   geometry: [`crate::well::head_of`] and the scene, the same reads as what
//!   draws the well;
//! - **what surrounds it** - the feature, its aliases, the system - is
//!   followed here, through the references;
//! - **the headers** are decided **once for the whole file**, never per
//!   wellbore: a wellbore that had only one of the two columns of kind `code`
//!   would call it `code`, another `GEOL`, and the union of the columns would
//!   no longer unite anything.
//!
//! Here and not in `energyml-export`, whose exporters **never see a store**:
//! this one walks one, like `scene::of_package`.
//!
//! No face decides: `energyml wells`, the front end and Python call [`of_store`]
//! and write what it returns.

use std::collections::{HashMap, HashSet};

use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::property;
use energyml_domain::wellbore_table::{self as wt, Column, From, Merged, WellTable};
use energyml_storage::ObjectSummary;

use crate::source::ValueSource;

/// What the export returns: the merged table, and what could not be said.
#[derive(Debug, Clone)]
pub struct Export {
    /// The headers and the rows, ready for [`wt::to_csv`].
    pub merged: Merged,
    /// Each wellbore, its headers already those of the file - what a caller
    /// merges alone to write one file per wellbore.
    pub tables: Vec<WellTable>,
    /// What was left out, wellbore by wellbore, **named**.
    pub notes: Vec<String>,
}

/// A property read, with the uri its summary gives it.
type UriAndObject = (String, Box<dyn DynObject>);

/// The wellbores of this package, in one table.
///
/// `wanted` designates the frames - uuid, uri or a piece of the title - and
/// empty means all of them. A frame that does not read is **named in the
/// notes** and the others carry on: a file of a hundred and twenty wellbores
/// does not fall over for one.
///
/// **A list of summaries and a `ValueSource`, never an `EpcStore`** - the rule
/// of `scene::of_package`, for the same reason: an ETP dataspace then reads
/// like a file.
pub fn of_store(summaries: &[ObjectSummary], source: &dyn ValueSource, wanted: &[String]) -> Export {
    let mut notes = Vec::new();

    // **The properties, filed once by what they hang on.** Re-reading the whole
    // package for every frame cost 120 × 1 680 reads on a producer's corpus; one
    // pass is enough, and the index serves all the frames.
    //
    // Bucketed by the uuid of the supporting frame, then told apart by the
    // **uri** the property's reference gives: two versions of one frame share
    // the bucket, and each takes only the properties whose reference does not
    // name the other.
    let mut by_support: HashMap<String, Vec<UriAndObject>> = HashMap::new();
    for summary in summaries
        .iter()
        .filter(|one| one.qualified_type.ends_with("Property"))
    {
        // By the **uri**: an object held in two versions shares its uuid, and
        // only the uri says which of the two this summary is.
        let Some(object) = source.object_at(&summary.uri) else {
            continue;
        };
        if property::read(object.as_reflect()).is_err() {
            continue;
        }
        if let Some(support) = crate::well::reference_of(object.as_reflect(), "SupportingRepresentation") {
            let bucket = energyml_core::identity::parse_address(&support).0;
            by_support.entry(bucket).or_default().push((support, object));
        }
    }

    let mut tables = Vec::new();
    for summary in summaries
        .iter()
        .filter(|one| one.qualified_type.contains("WellboreFrameRepresentation"))
    {
        let Some(frame) = source.object_at(&summary.uri) else {
            notes.push(format!("{}: the frame does not read", summary.uri));
            continue;
        };
        let title = energyml_core::identity::title_of_reflect(frame.as_reflect())
            .unwrap_or_default()
            .to_string();
        if !wanted.is_empty()
            && !wanted.iter().any(|one| {
                one.eq_ignore_ascii_case(&summary.uuid)
                    || *one == summary.uri
                    || (!one.is_empty() && title.contains(one.as_str()))
            })
        {
            continue;
        }
        let properties = by_support.get(&summary.uuid.to_ascii_lowercase());
        let borrowed: Vec<&dyn Reflect> = properties
            .map(|all| {
                all.iter()
                    .filter(|(support, _)| crate::well::same_uri(support, &summary.uri))
                    .map(|(_, one)| one.as_reflect())
                    .collect()
            })
            .unwrap_or_default();
        match wt::of_frame(frame.as_reflect(), &borrowed, source) {
            Ok(table) => {
                notes.extend(table.notes.iter().map(|one| format!("{}: {one}", table.well)));
                tables.push(around(
                    table,
                    frame.as_reflect(),
                    &summary.uri,
                    source,
                    &mut notes,
                ));
            }
            Err(why) => notes.push(format!("{}: {why}", summary.uri)),
        }
    }

    name_headers(&mut tables);
    let mut merged = in_order(wt::merge(&tables), &tables);
    // The wellbore name takes the title of the kind that designates it, like
    // the rest.
    if let Some(first) = merged.headers.first_mut() {
        *first = "well name".to_string();
    }
    Export {
        merged,
        tables,
        notes,
    }
}

/// The frame, with what the package says **around** it, in the order of the
/// borehole CSV: who the wellbore is, where it is, then each node.
fn around(
    table: WellTable,
    frame: &dyn Reflect,
    uri: &str,
    source: &dyn ValueSource,
    notes: &mut Vec<String>,
) -> WellTable {
    let rows = table.rows;
    let constant = |title: &str, what: &'static str, value: Option<String>| Column {
        title: title.to_string(),
        uom: None,
        kind: None,
        kind_title: Some(title.to_string()),
        from: From::Wellbore(what),
        values: vec![value; rows],
    };

    // Who: the feature, through the interpretation the frame represents.
    let interpretation = follow(source, frame, "RepresentedObject")
        .or_else(|| follow(source, frame, "RepresentedInterpretation"));
    let feature = interpretation
        .as_ref()
        .and_then(|one| follow(source, one.as_reflect(), "InterpretedFeature"));
    let well = feature
        .as_ref()
        .and_then(|one| energyml_core::identity::title_of_reflect(one.as_reflect()))
        .map(|title| wt::stem(title).to_string())
        .unwrap_or_else(|| table.well.clone());

    let mut columns: Vec<Column> = Vec::new();
    // The aliases: an industry code, a national identifier. One per column,
    // named by its authority - two of the same name are numbered.
    let mut seen: HashMap<String, usize> = HashMap::new();
    for owner in feature
        .iter()
        .map(|one| one.as_reflect())
        .chain(std::iter::once(frame))
    {
        for (authority, identifier) in aliases(owner) {
            let count = seen.entry(authority.clone()).or_default();
            *count += 1;
            let title = if *count == 1 {
                authority.clone()
            } else {
                format!("{authority} {count}")
            };
            let mut column = constant(&title, "alias", Some(identifier));
            column.kind_title = None;
            columns.push(column);
        }
    }

    // Where: the head, read at the datum - not at the first node, which is not
    // the ground when the first log starts lower down.
    let head = follow(source, frame, "Trajectory")
        .ok_or_else(|| "the frame names no trajectory".to_string())
        .and_then(|trajectory| {
            crate::well::head_of(trajectory.as_reflect(), source).map_err(|why| why.to_string())
        });
    match head {
        Ok([x, y, z]) => {
            columns.push(constant("x coordinate", "head", Some(number(x))));
            columns.push(constant("y coordinate", "head", Some(number(y))));
            columns.push(constant("elevation", "head", Some(number(z))));
        }
        Err(why) => notes.push(format!("{well}: no wellhead - {why}")),
    }

    // The system, and the coordinates of each node as the scene reads them: the
    // same read as what draws the well. **Their z follows the convention of
    // `well.rs`** - the file's own numbers when the trajectory has a geometry,
    // an elevation (upwards) when it has only a datum, never both in one
    // package.
    let placed = crate::scene::object_of(frame, source, uri);
    let mut nodes: Option<Vec<[f64; 3]>> = None;
    match &placed {
        Ok(object) => {
            if let Some(crs) = &object.crs {
                columns.push(constant(
                    "projection code",
                    "crs",
                    crs.projected.epsg().map(|code| code.to_string()),
                ));
                columns.push(constant(
                    "vertical projection code",
                    "crs",
                    crs.vertical.epsg().map(|code| code.to_string()),
                ));
            }
            let points: Vec<[f64; 3]> = object
                .geometry
                .patches
                .iter()
                .flat_map(|patch| patch.points.iter().copied())
                .collect();
            let by_passes = table
                .columns
                .first()
                .is_some_and(|one| one.from == From::TopDepth);
            if points.len() == rows {
                nodes = Some(points);
            } else if by_passes && points.len() == rows + 1 {
                // **A pass carries the location of its base**, as the borehole CSV
                // writes it: its `Z` is the ground minus `PROF`. The top node
                // is the previous row's, so nothing is lost.
                nodes = Some(points[1..].to_vec());
            } else {
                // A point count that is not the node count: laying them row by
                // row would shift every coordinate.
                notes.push(format!(
                    "{well}: {} placed point(s) for {rows} node(s), so no node coordinate is written",
                    points.len()
                ));
            }
        }
        Err(why) => notes.push(format!("{well}: the nodes are not placed - {why}")),
    }

    // The MD first: it is the axis - the top and base of each pass when the
    // table is organised by interval.
    let by_passes = table
        .columns
        .first()
        .is_some_and(|one| one.from == From::TopDepth);
    let axes = if by_passes { 2 } else { 1 };
    let mut properties = table.columns.into_iter();
    columns.extend(properties.by_ref().take(axes));
    if let Some(points) = nodes {
        // **Fixed names**, never a kind: `z coordinate` would go sometimes to
        // the node, sometimes to a `Z` property of the same kind, depending on
        // the package - and a header whose meaning changes from one file to
        // the next cannot be read back.
        for (axis, title) in [(0, "node x"), (1, "node y"), (2, "node z")] {
            columns.push(Column {
                title: title.to_string(),
                uom: None,
                kind: None,
                kind_title: None,
                from: From::Wellbore("node"),
                values: points.iter().map(|point| Some(number(point[axis]))).collect(),
            });
        }
    }
    columns.extend(properties);

    // And what it takes to find the objects again: a CSV that has lost its
    // uuids can be matched to the package only by guessing from titles.
    let mut frame_uuid = constant("wellbore frame uuid", "uuid", Some(table.uuid.clone()));
    frame_uuid.kind_title = None;
    columns.push(frame_uuid);
    if let Some(feature) = &feature {
        let mut feature_uuid = constant(
            "wellbore feature uuid",
            "uuid",
            energyml_core::identity::uuid_of_reflect(feature.as_reflect()).map(str::to_string),
        );
        feature_uuid.kind_title = None;
        columns.push(feature_uuid);
    }

    WellTable {
        well,
        uuid: table.uuid,
        columns,
        rows,
        notes: table.notes,
    }
}

/// The file in **a fixed order**: who, where, the system, the MD, the nodes,
/// the properties, the uuids.
///
/// `wt::merge` orders by first appearance, so a first wellbore without a head
/// put the uuids before the coordinates (measured on a fesapi package). The
/// rank comes from the nature of the column; at equal rank, the order of
/// appearance.
fn in_order(merged: Merged, tables: &[WellTable]) -> Merged {
    let rank = |from: &From| -> u8 {
        match from {
            From::Wellbore("alias") => 1,
            From::Wellbore("head") => 2,
            From::Wellbore("crs") => 3,
            From::TopDepth | From::MeasuredDepth => 4,
            From::Wellbore("node") => 5,
            From::Property(_) => 6,
            From::Wellbore(_) => 7,
        }
    };
    let mut of_header: HashMap<&str, u8> = HashMap::new();
    for table in tables {
        for column in &table.columns {
            of_header
                .entry(column.title.as_str())
                .or_insert_with(|| rank(&column.from));
        }
    }
    // The wellbore name stays first: it says whose the row is.
    let mut positions: Vec<usize> = (1..merged.headers.len()).collect();
    positions.sort_by_key(|at| {
        of_header
            .get(merged.headers[*at].as_str())
            .copied()
            .unwrap_or(u8::MAX)
    });
    let order: Vec<usize> = std::iter::once(0).chain(positions).collect();
    Merged {
        headers: order.iter().map(|at| merged.headers[*at].clone()).collect(),
        rows: merged
            .rows
            .iter()
            .map(|row| order.iter().map(|at| row.get(*at).cloned().flatten()).collect())
            .collect(),
        conflicts: merged.conflicts,
    }
}

/// The headers, decided **for the whole file**.
///
/// The title of the property kind when it designates only one column of the
/// file; the title of the property otherwise - three columns of kind
/// `formation name` would all be called the same, and the kind `unknown` says
/// nothing. The wellbore's own columns (head, system, nodes) go **first**:
/// they are the ones entitled to `x coordinate`, and a property of that kind
/// then takes its own title.
fn name_headers(tables: &mut [WellTable]) {
    // A column is identified by its nature and its title, in the order the
    // file meets it.
    let key = |column: &Column| -> String {
        match &column.from {
            From::Property(_) => format!("property:{}", column.title),
            From::MeasuredDepth => "md".to_string(),
            From::TopDepth => "top".to_string(),
            From::Wellbore(what) => format!("{what}:{}", column.title),
        }
    };
    let mut order: Vec<(String, String, Option<String>, bool)> = Vec::new();
    let mut known: HashSet<String> = HashSet::new();
    for table in tables.iter() {
        for column in &table.columns {
            let id = key(column);
            if known.insert(id.clone()) {
                let structural = !matches!(column.from, From::Property(_));
                order.push((id, column.title.clone(), column.kind_title.clone(), structural));
            }
        }
    }
    let mut claims: HashMap<String, usize> = HashMap::new();
    for (_, _, preferred, _) in &order {
        if let Some(preferred) = preferred {
            *claims.entry(preferred.clone()).or_default() += 1;
        }
    }
    // The wellbore's columns first, then the properties.
    let mut sorted: Vec<&(String, String, Option<String>, bool)> = order.iter().collect();
    sorted.sort_by_key(|(_, _, _, structural)| !structural);
    let mut used: HashSet<String> = HashSet::new();
    let mut header: HashMap<String, String> = HashMap::new();
    for (id, title, preferred, _) in sorted {
        let mut chosen = match preferred {
            Some(preferred) if claims.get(preferred) == Some(&1) && preferred != "unknown" => {
                preferred.clone()
            }
            _ => title.clone(),
        };
        if used.contains(&chosen) {
            chosen = title.clone();
        }
        let mut rank = 2;
        let base = chosen.clone();
        while used.contains(&chosen) {
            chosen = format!("{base} ({rank})");
            rank += 1;
        }
        used.insert(chosen.clone());
        header.insert(id.clone(), chosen);
    }
    for table in tables.iter_mut() {
        for column in &mut table.columns {
            if let Some(name) = header.get(&key(column)) {
                column.title = name.clone();
            }
        }
    }
}

/// The object a field references, when the package holds it.
fn follow(source: &dyn ValueSource, object: &dyn Reflect, field: &str) -> Option<Box<dyn DynObject>> {
    let FieldValue::Object(reference) = object.field(field)? else {
        return None;
    };
    crate::follow::object_of(source, reference)
}

/// The aliases of an object: `(header, identifier)`.
///
/// **The header is what the alias is, then who gives it** -
/// `industry code (BRGM/BSS)`, `regulatory identifier (BRGM/BSS)`: the SIGEOL
/// import writes three per wellbore, and `alias`, `alias 2`, `alias 3` did not
/// say which one is the BSS code. `authority` is an XML **attribute**, hence
/// its case; `IdentifierKind` is an extensible enumeration, read as text.
fn aliases(object: &dyn Reflect) -> Vec<(String, String)> {
    let Some(FieldValue::List(items)) = object.field("Aliases") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let FieldValue::Object(alias) = item else {
                return None;
            };
            let text = |field: &str| {
                alias
                    .field(field)
                    .and_then(|value| value.as_scalar())
                    .map(|one| {
                        one.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| one.to_string())
                    })
                    .filter(|one| !one.trim().is_empty())
            };
            let identifier = text("Identifier")?;
            let authority = text("authority").or_else(|| text("Authority"));
            let kind = text("IdentifierKind");
            let header = match (kind, authority) {
                (Some(kind), Some(authority)) => format!("{kind} ({authority})"),
                (Some(one), None) | (None, Some(one)) => one,
                (None, None) => "alias".to_string(),
            };
            Some((header, identifier))
        })
        .collect()
}

/// A number without useless zeros.
fn number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}
