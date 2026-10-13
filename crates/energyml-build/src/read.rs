//! An object of a package as pieces: its shape, and what is then known of it.
//!
//! The one place that turns a stored representation into a [`Source::Shape`]
//! and a [`Source::Object`] - the CLI's `--from model.epc#uuid` and the
//! front end's *create from this object* both call it. Two derivations of *what
//! the caller knows of an object* would be the shape of a bug once found here:
//! two readers agreeing on a uuid and disagreeing on which CRS is vertical.
//!
//! It reads: the geometry through the scene reader every export uses, the CRS
//! and interpretation it resolves, and - by reflection on the compound CRS -
//! its vertical half and direction, which a well's datum needs and a reader of
//! frames resolves to know which way is down.
//!
//! [`known_of_object`] is the same question asked of an object that **has no
//! geometry** - the CRS somebody just picked in a form. It is what makes an
//! answer behave like an input: *an answer is not an endpoint, it is one more
//! input*.

use energyml_storage::{EpcStore, ObjectSummary};

use crate::pieces::{Known, Ref, Source};

/// The entry `key` names - a full uri (`eml:///resqml22.X(uuid=…,version='2')`),
/// a short one, or a bare uuid - resolved by the store's one lookup. **A uuid
/// alone is the caller's grace, not this module's habit**: everything inside
/// reads by the summary's uri, so that an object held in two versions is read as
/// the version asked for, and a bare uuid that names several is refused with
/// its candidates (internally an object is always named by its full uri -
/// ADR-0003).
fn summary_for(store: &EpcStore, key: &str) -> Result<ObjectSummary, String> {
    match store.resolve(key) {
        Ok(summary) => Ok(summary.clone()),
        Err(energyml_storage::Error::NotFound(_)) => {
            Err(format!("no object of the package has the uuid {key}"))
        }
        Err(other) => Err(other.to_string()),
    }
}

/// **The entry a reference names**, by its full identity - uuid, type and the
/// `objectVersion` it states ([`EpcStore::resolve_reference`]): the one way a
/// reader of this crate follows a reference, so that an object held in two
/// versions is read as the version the reference means. A reference that names
/// no type is looked up by what it does name, and an uuid that leaves several
/// candidates answers `None` - the checker says why, this reader does not guess.
fn followed<'a>(
    store: &'a EpcStore,
    reference: &dyn energyml_core::value::Reflect,
) -> Option<&'a ObjectSummary> {
    match energyml_core::identity::ObjectId::of_reflect(reference) {
        Some(id) => store
            .resolve_reference(&id.uuid, Some(&id.qualified_type()), id.object_version.as_deref())
            .ok(),
        None => store
            .lookup_uuid(energyml_core::identity::uuid_of_reflect(reference)?)
            .ok(),
    }
}

/// The entry a [`Ref`] names - [`followed`] for what a piece already holds.
fn summary_of_ref<'a>(store: &'a EpcStore, one: &Ref) -> Option<&'a ObjectSummary> {
    let qualified_type = Some(one.qualified_type.as_str()).filter(|text| !text.is_empty());
    store
        .resolve_reference(&one.uuid, qualified_type, one.object_version.as_deref())
        .ok()
}

/// What `holder` says of the object `uuid`: the reference it holds to it -
/// type and version -, found among its own references. `None` when `holder`
/// holds none to that uuid (a CRS a frame borrows from its trajectory).
fn reference_held(holder: &dyn energyml_core::value::Reflect, uuid: &str) -> Option<Ref> {
    energyml_core::identity::collect_refs(holder)
        .into_iter()
        .find(|edge| edge.target.uuid.eq_ignore_ascii_case(uuid))
        .map(|edge| {
            Ref::new(edge.target.uuid, edge.target.qualified_type(), "")
                .with_version(edge.target.object_version.map(str::to_string))
        })
}

/// The entry `holder` names by `uuid`: by the reference it holds to it when it
/// holds one, by the uuid alone - [`EpcStore::lookup_uuid`], which refuses a
/// uuid that names several - when it holds none.
fn held<'a>(
    store: &'a EpcStore,
    holder: &dyn energyml_core::value::Reflect,
    uuid: &str,
) -> Option<&'a ObjectSummary> {
    match reference_held(holder, uuid) {
        Some(reference) => summary_of_ref(store, &reference),
        None => store.lookup_uuid(uuid).ok(),
    }
}

/// The two pieces an object of the package makes: `(name, shape)` and
/// `(name (object), known)`.
///
/// `uuid` may be a bare uuid or a uri - **a uri when the package may hold the
/// object in several versions**; the object must read as a geometry -
/// a representation with arrays the store can reach - or the error names why
/// not, the array backend included.
///
/// The shape is in the object's **local** frame: what a plan that *writes*
/// needs, because RESQML geometry is stored local. A plan computing for the
/// **scene** reads it placed - see [`pieces_of_object_in`].
pub fn pieces_of_object(store: &EpcStore, key: &str) -> Result<Vec<(String, Source)>, String> {
    pieces_of_object_in(store, key, energyml_geometry::Placement::Local)
}

/// The same, the shape **placed** as asked.
///
/// # Why a plan for the scene reads the projected placement
///
/// An artefact created from an object whose Z is in time was reported
/// mis-positioned in Z.
/// The scene draws the projected placement and holds its artefacts in it; a
/// geometry computed from the local numbers and kept as an artefact landed
/// where the file's numbers are - offsets, rotation and, for a CRS counting
/// downward (every time CRS, most depth ones), the **vertical flip** missing.
/// Every input of a geometry plan must be in the one frame the scene uses, or
/// the computation mixes frames; an artefact input is already placed.
///
/// `vertical_up` then says **up**: `to_projected` has flipped a downward axis,
/// so the placed z is an elevation whatever the file counted - and an
/// operation that reads the direction (the curvature signs of `surface-map`)
/// must be told what it is actually given.
pub fn pieces_of_object_in(
    store: &EpcStore,
    key: &str,
    placement: energyml_geometry::Placement,
) -> Result<Vec<(String, Source)>, String> {
    let summary = summary_for(store, key)?;
    let object = store
        .get_object(&summary.uri)
        .map_err(|e| format!("reading {}: {e}", summary.uri))?;
    let built = energyml_geometry::scene::object_of(object.as_reflect(), store, &summary.uri)
        .map_err(|e| format!("{}: no geometry could be read - {e}", summary.uri))?;

    let crs = built.crs.as_ref().and_then(|crs| {
        // By the reference the object holds to it (version included), by the uuid
        // alone only when it holds none - a frame borrowing its trajectory's.
        held(store, object.as_reflect(), &crs.uuid)
            // **The object's own title**, the uri only when it has none. A
            // reference's title is a copy, and a copy that differs is what
            // `check` calls stale - on every object written against a CRS of
            // the package (ten warnings on the contacts of a real package).
            .map(|found| Ref::of_summary(found, titled(store, found)))
    });
    let interpretation = built.interpretation.as_ref().map(|related| {
        Ref::new(
            related.uuid.clone(),
            related.qualified_type.clone(),
            related.title.clone(),
        )
        .with_version(reference_held(object.as_reflect(), &related.uuid).and_then(|held| held.object_version))
    });
    // The trajectory and the datum of a frame: what is needed to **continue**
    // a well rather than write a copy of it.
    let (trajectory, datum) = well_refs_of(store, object.as_reflect());
    // The frame's vertical half, read off the compound CRS by reflection: what
    // a datum must reference. Found through a compound, or through the
    // compound a reference point is written in (`vertical_half`). Absent when
    // the frame names a bare `ProjectedCrs` - which names no vertical system,
    // and one is not guessed from the package - or a vertical system the
    // package does not hold: *not known*, never *none*.
    let vertical_crs = built.crs.as_ref().and_then(|crs| {
        let system = held(store, object.as_reflect(), &crs.uuid)?;
        let system = store.get_object(&system.uri).ok()?;
        vertical_half(store, system.as_reflect())
    });
    let placed = !matches!(placement, energyml_geometry::Placement::Local);
    let vertical_up = built.crs.as_ref().map(|crs| {
        placed
            || matches!(
                crs.vertical_direction,
                energyml_domain::crs::VerticalDirection::Up
            )
    });
    let name = if built.title.is_empty() {
        summary.uuid.clone()
    } else {
        built.title.clone()
    };
    let node_count = Some(built.geometry.point_count());
    // **What the file says of closure travels with the shape.** The reader
    // reads it (`ClosedPolylines`); without this line, copying a closed
    // polyline of the package would write an open one, and the operation would
    // have to *guess* it from the coincidence of the two ends - which it can
    // do and notes, but which has no place when the file states it. `None`
    // when no patch says anything.
    let closed: Vec<bool> = built
        .geometry
        .patches
        .iter()
        .flat_map(|patch| patch.closed.clone())
        .collect();
    let closed = if closed.is_empty() { None } else { Some(closed) };
    let geometry = built
        .geometry_in(placement)
        .map_err(|e| format!("{}: {e}", summary.uri))?;
    Ok(vec![
        (name.clone(), Source::Shape { geometry, closed }),
        (
            format!("{name} (object)"),
            Source::Object(Known {
                reference: Some(Ref::of_summary(&summary, name)),
                // The codes the frame names, when it names any. `energyml-domain`
                // has already told the four cases apart - a code, a local
                // authority, an explicit *unknown*, nothing at all - so this is
                // a copy and not a second reading of the same fact.
                crs_epsg: built
                    .crs
                    .as_ref()
                    .and_then(|crs| crs.projected.epsg())
                    .map(|code| code as i64),
                vertical_epsg: built
                    .crs
                    .as_ref()
                    .and_then(|crs| crs.vertical.epsg())
                    .map(|code| code as i64),
                crs,
                interpretation,
                vertical_uom: built.crs.as_ref().and_then(|crs| crs.vertical_uom.clone()),
                vertical_crs,
                vertical_up,
                placement: if placed { None } else { built.crs.clone() },
                node_count,
                wellhead: wellhead_of(store, object.as_reflect()),
                node_mds: node_mds_of(store, object.as_reflect()),
                categories: categories_of(store, object.as_reflect()),
                interval_units: interval_units_of(store, object.as_reflect()),
                trajectory,
                datum,
                // A representation is no property: what a property states is
                // asked of `known_of_object`, which a property reaches.
                property: None,
                // Nor a unit or a column.
                stratigraphy: None,
                // Nor a time series.
                instants: None,
                json: Some(energyml_core::json::write_value(object.as_reflect())),
                path: path_of(store, &summary, object.as_reflect()),
            }),
        ),
    ])
}

/// The pieces of an object, named for the **slot** that received it: the
/// shape takes the slot's name, what is known of it `<slot> (object)`.
///
/// # Why a slot's name and not the object's title
///
/// An operation reads its roles by the names of its pieces - `trace` and
/// `surfaces` for `cross-section`, `fault` for `intersect`, `faults` for the
/// barriers of `interpolate`, `top` and `base` for `thickness`. An object of
/// the package came named by its title, so none of them saw it: `intersect` on
/// a fault and a horizon of the package was unfit, and a fault of the package
/// given as `faults` was no barrier - silently. The one place the
/// renaming is written; the front end and the CLI both come through here.
pub fn in_slot(slot: &str, read: Vec<(String, Source)>) -> Vec<(String, Source)> {
    read.into_iter()
        .map(|(_, source)| {
            let name = match source {
                Source::Object(_) => format!("{slot} (object)"),
                _ => slot.to_string(),
            };
            (name, source)
        })
        .collect()
}

/// The bag, with every slot that declares [`Otherwise::Every`] and was left
/// empty filled from `store` - the front end and the CLI both come through here.
///
/// An empty slot is one no piece is named after: neither `wells` nor
/// `wells 2`. It then receives every object of `store` whose type the slot
/// [accepts](crate::operation::Takes::accepts), each as
/// [`pieces_of_object_in`] reads it - a shape and its companion, named
/// `wells`, `wells 2`, … - or, for an object that is no representation, what
/// [`known_of_object`] knows of it.
///
/// **A representation that cannot be read is an error, not a gap**: a build
/// with no array backend must say so rather than import as if the package held
/// no well - after which every hole would be written a second time.
///
/// [`Otherwise::Every`]: crate::operation::Otherwise::Every
pub fn every(
    store: &EpcStore,
    slots: &[crate::operation::Slot],
    bag: crate::pieces::Pieces,
    placement: energyml_geometry::Placement,
) -> Result<crate::pieces::Pieces, crate::operation::Refusal> {
    use crate::operation::{Need, Otherwise, Refusal};

    let mut bag = bag;
    let _span = tracing::info_span!("read.every", slots = slots.len()).entered();
    for slot in slots {
        let (Otherwise::Every, Need::Input(takes)) = (&slot.otherwise, &slot.need) else {
            continue;
        };
        let named = |name: &str| {
            name == slot.name
                || name
                    .strip_prefix(slot.name)
                    .and_then(|rest| rest.strip_prefix(' '))
                    .is_some_and(|rest| rest.parse::<usize>().is_ok())
        };
        if bag.iter().any(|(name, _)| named(name)) {
            continue;
        }
        let mut rank = 0usize;
        for summary in store
            .resources()
            .into_iter()
            .filter(|one| takes.accepts(&one.qualified_type))
        {
            rank += 1;
            let name = if rank == 1 {
                slot.name.to_string()
            } else {
                format!("{} {rank}", slot.name)
            };
            // The line is drawn on the **type**, as the front end draws it: a
            // representation that does not read is a failure, an object that
            // is none has nothing to fail.
            // By the **uri**: the summary in hand names this entry alone, a uuid
            // names every version of it (Lutetia's « H3 triangulée » held twice
            // was read as the original both times).
            let read = match pieces_of_object_in(store, &summary.uri, placement) {
                Ok(read) => in_slot(&name, read),
                // **A refusal filed under its slot**, not a raised error: the
                // front end shows refusals under the field they concern, and a raised
                // error would lose its structure crossing the worker.
                Err(why) if takes.kind != "object" || summary.qualified_type.ends_with("Representation") => {
                    return Err(Refusal::on(slot.name, why));
                }
                Err(_) => {
                    let known = known_of_object_reached(store, &summary.uri, Reach::Every)
                        .map_err(|why| Refusal::on(slot.name, why))?;
                    vec![(name, Source::Object(known))]
                }
            };
            for (name, source) in read {
                bag = bag.with(name, source);
            }
        }
    }
    Ok(bag)
}

/// The frames of the package a geometry of `projected` can be written in
/// instead of creating one - every `LocalEngineeringCompoundCrs` read as it
/// states itself, judged by [`crate::crs::compatible_frames`].
pub fn compatible_frames(store: &EpcStore, projected: i64) -> Vec<Known> {
    let candidates: Vec<Known> = store
        .resources()
        .into_iter()
        .filter(|one| {
            matches!(
                crate::crs::frame_kind(&one.qualified_type),
                Ok(crate::crs::FrameKind::Compound)
            )
        })
        .filter_map(|one| known_of_object(store, &one.uri).ok())
        .collect();
    crate::crs::compatible_frames(&candidates, projected)
        .into_iter()
        .cloned()
        .collect()
}

/// What is known of an object **without** asking it for a geometry.
///
/// # Why this is not `pieces_of_object`
///
/// That one reads a representation, and a CRS is not one: `object_of` refuses
/// it, rightly, because it has no points. But a form that has just been given a
/// CRS wants exactly what that CRS *says* - its projected and vertical EPSG
/// codes, so that it stops asking for them.
///
/// Four cases, in order, and none of them is an error:
///
/// - it **is a property** - decided on its type: then what it states, its
///   values included when it hangs on a wellbore frame
///   ([`crate::pieces::Known::property`]), so that an operation can carry it
///   (`contacts-from-frames`);
/// - it **reads as a coordinate system**: then it *is* the frame, and the facts
///   are its own - that is what makes `common_slots` suggest `project` and
///   `vertical` from a system somebody named;
/// - it reads as a **geometry**: the same `Known` `pieces_of_object` builds,
///   without the shape;
/// - it reads as neither: the object exists and nothing more is known. A
///   reference and nothing else, never a refusal - a slot fed an object of an
///   unexpected sort must fail on the slot, by name, not here.
///
/// `key` is **the object's uri** (a bare uuid is the caller's grace, refused
/// when the package holds it twice), and the reference in the answer carries the
/// version the entry declares ([`Ref::of_summary`]).
pub fn known_of_object(store: &EpcStore, key: &str) -> Result<Known, String> {
    known_of_object_reached(store, key, Reach::Named)
}

/// How an object came to be read - which decides whose property values are.
///
/// See [`property_of`]: a property somebody **named** has its values read
/// whatever it hangs on; one met while filling a slot with *every* property of
/// the package has them read only on a wellbore frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Named by a recipe entry, picked in the front end, passed on the command line.
    Named,
    /// Met by [`every`], one object of the package after another.
    Every,
}

/// [`known_of_object`], reached as said.
fn known_of_object_reached(store: &EpcStore, key: &str, reach: Reach) -> Result<Known, String> {
    let mut known = known_of_object_but_whole(store, key, reach)?;
    // The object whole (`Known::json`), whichever branch answered - the
    // geometry branch has already read it.
    if known.json.is_none()
        && let Ok(summary) = summary_for(store, key)
        && let Ok(object) = store.get_object(&summary.uri)
    {
        known.json = Some(energyml_core::json::write_value(object.as_reflect()));
    }
    Ok(known)
}

/// [`known_of_object`], all but [`Known::json`].
fn known_of_object_but_whole(store: &EpcStore, key: &str, reach: Reach) -> Result<Known, String> {
    let summary = summary_for(store, key)?;
    let object = store
        .get_object(&summary.uri)
        .map_err(|e| format!("reading {}: {e}", summary.uri))?;
    let title = energyml_core::identity::title_of_reflect(object.as_reflect())
        .unwrap_or(&summary.uri)
        .to_string();
    let reference = Some(Ref::of_summary(&summary, title));

    // **A property first, and decided on its type.** Before the CRS branch,
    // because a 2.2 property may name a `LocalCrs` and `crs::read_with` would
    // then answer for it - a property taken for a coordinate system, which
    // `suggest_written` would offer as the frame of what is written.
    if is_property(&summary.qualified_type) {
        return Ok(Known {
            property: Some(property_of(store, object.as_reflect(), reach)),
            reference,
            ..Known::default()
        });
    }

    // **A unit or a column: the stratigraphy around it.** The whole of it,
    // because what a unit contains is stated by the finer ranks of its column
    // and by the models already made of it - none of which it names.
    if is_stratigraphy(&summary.qualified_type) {
        return Ok(Known {
            stratigraphy: Some(stratigraphy_of(store)),
            reference,
            ..Known::default()
        });
    }

    // **A time series: its instants**, in their order - what a property laid
    // through it names by place (`Known::instants`).
    if summary
        .qualified_type
        .rsplit('.')
        .next()
        .map(|class| class.trim_start_matches("obj_"))
        == Some("TimeSeries")
    {
        return Ok(Known {
            instants: instants_of(object.as_reflect()),
            reference,
            ..Known::default()
        });
    }

    if let Ok(crs) = energyml_domain::crs::read_with(object.as_reflect(), store) {
        let vertical_crs = vertical_half(store, object.as_reflect());
        return Ok(Known {
            crs: reference.clone(),
            crs_epsg: crs.projected.epsg().map(|code| code as i64),
            vertical_epsg: crs.vertical.epsg().map(|code| code as i64),
            vertical_uom: crs.vertical_uom.clone(),
            vertical_up: Some(matches!(
                crs.vertical_direction,
                energyml_domain::crs::VerticalDirection::Up
            )),
            vertical_crs,
            placement: Some(crs.clone()),
            reference,
            ..Known::default()
        });
    }

    match pieces_of_object(store, &summary.uri) {
        Ok(pieces) => Ok(pieces
            .into_iter()
            .find_map(|(_, source)| match source {
                Source::Object(known) => Some(known),
                _ => None,
            })
            .unwrap_or(Known {
                reference,
                ..Known::default()
            })),
        Err(_) => Ok(Known {
            reference,
            ..Known::default()
        }),
    }
}

/// The `DateTime` of each `Time` of a time series, in the list's order - the
/// same field in eml 2.0 and 2.3 (`GeologicTime.DateTime`). `None` when one of
/// them states none: a list with a hole would shift every index after it.
fn instants_of(reflect: &dyn energyml_core::value::Reflect) -> Option<Vec<String>> {
    use energyml_core::value::{FieldValue, Scalar};
    let times: Vec<&dyn energyml_core::value::Reflect> = match reflect.field("Time")? {
        FieldValue::List(items) => items.iter().map(FieldValue::as_object).collect::<Option<_>>()?,
        FieldValue::Object(one) => vec![one],
        _ => return None,
    };
    times
        .into_iter()
        .map(|time| match time.field("DateTime")?.as_scalar()? {
            Scalar::Str(text) => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

/// The distinct values a category table holds, in the order of its rows.
///
/// **The last column that is not a key**, the same rule as
/// `energyml_domain::wellbore_table` applies to read a label: a
/// `StringTableLookup` has `Key` then `Value`, a category table has the code
/// then its name, and a one-column table *is* the list.
///
/// `None` for everything else - and an unreadable table answers `None` too:
/// continuing a table that could not be read would write codes against an
/// order that was guessed.
fn categories_of(store: &EpcStore, reflect: &dyn energyml_core::value::Reflect) -> Option<Vec<String>> {
    let read = energyml_domain::table::Lookup::read(reflect, store).ok()?;
    let column = (0..read.titles.len())
        .rev()
        .find(|one| !read.key_columns.contains(one))?;
    (0..read.rows.rows)
        .map(|row| match read.rows.cell(row, column)? {
            energyml_domain::table::Cell::Text(text) => Some(text),
            energyml_domain::table::Cell::Number(number) => Some(format!("{number}")),
            energyml_domain::table::Cell::Boolean(flag) => Some(flag.to_string()),
        })
        .collect()
}

/// Whether a qualified type names a property: its class ends in `Property` -
/// `ContinuousProperty`, `DiscreteProperty`, `obj_CategoricalProperty`, … -
/// which neither `PropertyKind` nor `PropertySet` does.
/// A stratigraphic unit or a stratigraphic column, in any release.
fn is_stratigraphy(qualified_type: &str) -> bool {
    matches!(
        qualified_type
            .rsplit('.')
            .next()
            .unwrap_or(qualified_type)
            .trim_start_matches("obj_"),
        "StratigraphicUnitInterpretation" | "StratigraphicColumn"
    )
}

/// The package's columns, ranks, units and earth models, read once - only the
/// objects `stratigraphy::columns` looks at, so a package of two thousand wells
/// does not read two thousand trajectories to answer.
fn stratigraphy_of(store: &EpcStore) -> energyml_domain::stratigraphy::Columns {
    const READ: &[&str] = &[
        "StratigraphicColumn",
        "StratigraphicColumnRankInterpretation",
        "StratigraphicUnitInterpretation",
        "EarthModelInterpretation",
    ];
    let held: Vec<_> = store
        .resources()
        .iter()
        .filter(|summary| {
            let name = summary
                .qualified_type
                .rsplit('.')
                .next()
                .unwrap_or(&summary.qualified_type);
            READ.contains(&name.trim_start_matches("obj_"))
        })
        .filter_map(|summary| store.get_object(&summary.uri).ok())
        .collect();
    energyml_domain::stratigraphy::columns(held.iter().map(|object| object.as_reflect()))
}

fn is_property(qualified_type: &str) -> bool {
    qualified_type
        .rsplit('.')
        .next()
        .unwrap_or(qualified_type)
        .ends_with("Property")
}

/// What a property states, for an operation that carries it elsewhere - see
/// [`crate::pieces::KnownProperty`].
///
/// Everything is read through `energyml_domain::property::read`, the one
/// reader of a property in both releases, and the values through
/// `energyml_domain::values` behind it; the references are the ones the
/// property writes, kept whole (uuid, qualified type, title) so that a copy
/// references **the same** kind and **the same** table.
///
/// # Whose values are read: the named, and a frame's
///
/// A slot left empty to take *every property of the package* (`read::every`)
/// lands here one object at a time - so reading every array here would read
/// every array of the package for an operation that wanted a frame's. A
/// frame's array is a log's length; a grid's can be millions of values. Met
/// that way ([`Reach::Every`]), the values are read for a property that hangs
/// on a `WellboreFrameRepresentation`, and every other one **says why it was
/// not read** rather than arriving empty.
///
/// A property somebody **named** ([`Reach::Named`]) is one property, chosen:
/// its values are read whatever it hangs on. That is what lets
/// `property-time-series` and `zone-statistics` take a grid's property from a
/// recipe (the grid series of Lutetia named this rule as their second
/// blocker). Which elements the values lie on stays each operation's to judge:
/// they match a property on its supporting uuid, never on a count.
fn property_of(
    store: &EpcStore,
    reflect: &dyn energyml_core::value::Reflect,
    reach: Reach,
) -> crate::pieces::KnownProperty {
    use energyml_core::value::{FieldValue, Scalar};

    let read = match energyml_domain::property::read(reflect) {
        Ok(read) => read,
        Err(why) => {
            return crate::pieces::KnownProperty {
                supporting: reference_at(reflect, "SupportingRepresentation"),
                indexable: String::new(),
                value_count: Vec::new(),
                timed: false,
                realizations: false,
                uom: None,
                kind: None,
                lookup: None,
                null_value: None,
                facets: Vec::new(),
                values: Err(format!("it does not read as a property - {why}")),
            };
        }
    };
    let supporting = reference_at(reflect, "SupportingRepresentation");
    // The reference the property writes, when it writes one: a 2.0.1 standard
    // kind is a name of the vocabulary, and a name is no object to point at.
    let kind = match &read.kind {
        Some(energyml_domain::property::PropertyKindRef::Object { .. }) => {
            reference_at(reflect, "PropertyKind")
        }
        _ => None,
    };
    let lookup = reference_at(reflect, "CategoryLookup").or_else(|| reference_at(reflect, "Lookup"));
    // The null an integer array declares. Read off the array itself: RESQML
    // 2.2 puts it there (`eml23.IntegerExternalArray.NullValue`), and a
    // `DiscreteProperty` has no field of its own for it.
    let null_value =
        read.patches
            .first()
            .and_then(|patch| match patch.array.field("NullValue")?.as_scalar()? {
                Scalar::Int(value) => Some(value),
                Scalar::UInt(value) => i64::try_from(value).ok(),
                _ => None,
            });
    // The qualifiers, word for word, in the encoding a copy is written in.
    let facets = match reflect.field("Facet") {
        Some(FieldValue::List(items)) => items
            .iter()
            .filter_map(FieldValue::as_object)
            .map(energyml_core::json::write_value)
            .collect(),
        Some(FieldValue::Object(one)) => vec![energyml_core::json::write_value(one)],
        _ => Vec::new(),
    };
    let values = match supporting.as_ref().and_then(|one| summary_of_ref(store, one)) {
        None => Err(
            "its supporting representation is not in the package, so nothing says what its values hang on"
                .to_string(),
        ),
        Some(found)
            if reach == Reach::Every && !found.qualified_type.ends_with("WellboreFrameRepresentation") =>
        {
            Err(format!(
                "its values are not read when every property of the package is taken: it hangs on a `{}`, and only a wellbore frame's are - a log's length, where another representation's can be millions; name it to have them read",
                found.qualified_type
            ))
        }
        Some(found) => match read.patches.as_slice() {
            [one] => one.read(store).map_err(|why| why.to_string()),
            many => Err(format!(
                "it states {} value array(s) on its `{}`, and which elements each covers is not read here - one patch is",
                many.len(),
                found.qualified_type
            )),
        },
    };
    crate::pieces::KnownProperty {
        supporting,
        indexable: read.indexable.as_str().to_string(),
        value_count: read.value_count_per_element.clone(),
        timed: read.time.is_some(),
        realizations: read.has_realization_indices,
        uom: read.uom.clone(),
        kind,
        lookup,
        null_value,
        facets,
        values,
    }
}

/// The reference a field holds, as the file writes it - uuid, qualified type,
/// title, and the version it names. `None` when the field is absent or names no
/// uuid.
fn reference_at(reflect: &dyn energyml_core::value::Reflect, field: &str) -> Option<Ref> {
    let energyml_core::value::FieldValue::Object(reference) = reflect.field(field)? else {
        return None;
    };
    let uuid = energyml_core::identity::uuid_of_reflect(reference)?.to_string();
    let text = |name: &str| {
        reference
            .field(name)
            .and_then(|value| value.as_scalar())
            .and_then(|scalar| scalar.as_str().map(str::to_string))
            .unwrap_or_default()
    };
    // The type and the version as the reference's identity states them
    // (`ObjectId::of_reflect`): a 2.0 reference names its type by a
    // `ContentType`, which the field `QualifiedType` alone would miss.
    let id = energyml_core::identity::ObjectId::of_reflect(reference);
    let qualified_type = id
        .as_ref()
        .map(|id| id.qualified_type())
        .unwrap_or_else(|| text("QualifiedType"));
    Some(Ref::new(uuid, qualified_type, text("Title")).with_version(id.and_then(|id| id.object_version)))
}

/// The unit of each interval of a wellbore frame, from its
/// `IntervalStratigraphicUnits` (TRG 2.2 §5.34).
///
/// Each correspondence names a `StratigraphicColumnRankInterpretation` and
/// gives, per interval, an index into its `StratigraphicUnits` - through a
/// jagged array, *« since a single interval may correspond to several
/// units »*. Several correspondences (one per rank) are **merged**: an interval
/// takes the unit the one correspondence that names it gives. Two that name two
/// different units for one interval are not decided between: the answer is
/// `None` for the whole frame, and the operation that needs it says why.
fn interval_units_of(
    store: &EpcStore,
    reflect: &dyn energyml_core::value::Reflect,
) -> Option<Vec<Option<Ref>>> {
    use energyml_core::value::FieldValue;
    let maps = match reflect.field("IntervalStratigraphicUnits")? {
        FieldValue::List(items) => items,
        one @ FieldValue::Object(_) => vec![one],
        _ => return None,
    };
    let mut out: Option<Vec<Option<Ref>>> = None;
    for map in maps {
        let FieldValue::Object(map) = map else { continue };
        let FieldValue::Object(reference) = map.field("StratigraphicOrganizationInterpretation")? else {
            continue;
        };
        let found = followed(store, reference)?;
        let rank = store.get_object(&found.uri).ok()?;
        // The rank's units, in the order an index counts in.
        let units: Vec<Ref> = match rank.as_reflect().field("StratigraphicUnits") {
            Some(FieldValue::List(items)) => items
                .into_iter()
                .filter_map(|item| match item {
                    FieldValue::Object(unit) => {
                        let uuid = energyml_core::identity::uuid_of_reflect(unit)?.to_string();
                        let title = unit
                            .field("Title")
                            .and_then(|one| one.as_scalar())
                            .map(|one| one.to_string())
                            .unwrap_or_default();
                        // The version the rank states for the unit: two versions of a
                        // unit are two units of the column.
                        let version = energyml_core::identity::ObjectId::of_reflect(unit)
                            .and_then(|id| id.object_version);
                        Some(
                            Ref::new(uuid, "resqml22.StratigraphicUnitInterpretation", title)
                                .with_version(version),
                        )
                    }
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let FieldValue::Object(jagged) = map.field("UnitIndices")? else {
            continue;
        };
        let (Some(FieldValue::Object(elements)), Some(FieldValue::Object(lengths))) =
            (jagged.field("Elements"), jagged.field("CumulativeLength"))
        else {
            continue;
        };
        let elements =
            energyml_geometry::values::read_integers(elements, store, "UnitIndices/Elements").ok()?;
        let lengths =
            energyml_geometry::values::read_integers(lengths, store, "UnitIndices/CumulativeLength").ok()?;
        let mut start = 0usize;
        let mut these: Vec<Option<Ref>> = Vec::with_capacity(lengths.len());
        for end in &lengths {
            let end = usize::try_from(*end).ok()?.min(elements.len());
            // The first index of the interval that names a unit of the rank;
            // a null - negative, or past the list - names none.
            let unit = elements
                .get(start..end)
                .unwrap_or_default()
                .iter()
                .find_map(|index| usize::try_from(*index).ok().and_then(|at| units.get(at).cloned()));
            these.push(unit);
            start = end;
        }
        out = Some(match out {
            None => these,
            Some(before) => {
                if before.len() != these.len() {
                    return None;
                }
                let mut merged = Vec::with_capacity(before.len());
                for (one, other) in before.into_iter().zip(these) {
                    merged.push(match (one, other) {
                        (Some(a), Some(b)) if !a.same_object(&b) => return None,
                        (Some(a), _) => Some(a),
                        (None, b) => b,
                    });
                }
                merged
            }
        });
    }
    out
}

/// The measured depths a wellbore frame states for its nodes.
///
/// `None` for anything else - and `None` too when the array cannot be read,
/// which is what a build without an array backend gives. The two are the same
/// answer here (*this object does not tell us*) and they are told apart by the
/// refusal of whoever asked, never by an empty vector: a frame with no node is
/// a different thing from a frame we could not read.
fn node_mds_of(store: &EpcStore, reflect: &dyn energyml_core::value::Reflect) -> Option<Vec<f64>> {
    // The field, read by reflection: `field_object` of `energyml-geometry` is
    // private, and opening it to a caller would widen a surface for one need.
    // Two lines here say the same thing.
    let energyml_core::value::FieldValue::Object(node_md) = reflect.field("NodeMd")? else {
        return None;
    };
    energyml_geometry::values::read_doubles(node_md, store, "NodeMd").ok()
}

/// Where an object is **collared**, when the package states it.
///
/// Three hops at most, and they are the ones RESQML draws: a
/// `WellboreFrameRepresentation` names its `Trajectory`, a
/// `WellboreTrajectoryRepresentation` names a `Datum` inside its `MdInterval`,
/// and a `ReferencePointInACrs` *is* the answer.
///
/// **This is not the geometry**, and it cannot be: a trajectory starts at the
/// first stated depth, so its first node is not the ground - on a well whose
/// log starts at 31.5 m, reading it there would answer thirty metres too low.
/// The datum is the only thing that states it.
///
/// `None` everywhere else, and that is an answer: *this object does not say
/// where it is collared*, never a zero.
///
/// **A reference point in a local engineering compound is placed**.
/// Its coordinates are local ones
/// of the compound its `Crs` names (common TRG 2.3 §13.25 p. 371), so the
/// answer is [`energyml_domain::crs::Crs::to_projected`] of them: projected x
/// and y, an elevation in the compound's vertical unit - the frame a
/// `ReferencePointInACrs` is written in when its vertical CRS counts up, which
/// is what the builders here write. A compound that cannot be read, or that
/// counts time, answers `None`: a collar that cannot be placed is not known,
/// and handing back its local numbers would put it where the compound's
/// origin is not.
fn wellhead_of(store: &EpcStore, reflect: &dyn energyml_core::value::Reflect) -> Option<[f64; 3]> {
    fn number(reflect: &dyn energyml_core::value::Reflect, field: &str) -> Option<f64> {
        reflect.field(field)?.as_scalar()?.as_f64()
    }
    fn follow(
        store: &EpcStore,
        reflect: &dyn energyml_core::value::Reflect,
        field: &str,
    ) -> Option<Box<dyn energyml_core::registry::DynObject>> {
        let energyml_core::value::FieldValue::Object(reference) = reflect.field(field)? else {
            return None;
        };
        let found = followed(store, reference)?;
        store.get_object(&found.uri).ok()
    }

    // The point itself.
    if let (Some(z), Some(horizontal)) = (
        number(reflect, "VerticalCoordinate"),
        reflect.field("HorizontalCoordinates"),
    ) && let energyml_core::value::FieldValue::Object(coordinates) = horizontal
    {
        let x = number(coordinates, "Coordinate1")?;
        let y = number(coordinates, "Coordinate2")?;
        return match energyml_domain::crs::compound_of_datum(reflect, store) {
            Ok(Some(compound)) if !compound.is_time() => Some(compound.to_projected([x, y, z])),
            Ok(Some(_)) | Err(_) => None,
            Ok(None) => Some([x, y, z]),
        };
    }
    // A trajectory: its `MdInterval` names the datum.
    if let Some(energyml_core::value::FieldValue::Object(interval)) = reflect.field("MdInterval")
        && let Some(datum) = follow(store, interval, "Datum")
    {
        return wellhead_of(store, datum.as_reflect());
    }
    // A frame: its trajectory names it.
    let trajectory = follow(store, reflect, "Trajectory")?;
    wellhead_of(store, trajectory.as_reflect())
}

/// **The hole a wellbore object runs along** - [`crate::pieces::Known::path`]:
/// a trajectory's own line, or the line of the trajectory a frame names,
/// through `energyml_geometry::well::path_of` and nothing else.
///
/// `None` for what is neither - a type test on the class name, the release
/// and the `obj_` of 2.0.1 aside. A trajectory whose line cannot be evaluated
/// is `Some`, its reason in the `path`.
fn path_of(
    store: &EpcStore,
    summary: &energyml_storage::resource::ObjectSummary,
    reflect: &dyn energyml_core::value::Reflect,
) -> Option<crate::pieces::KnownPath> {
    use energyml_core::value::FieldValue;
    let class = summary
        .qualified_type
        .rsplit('.')
        .next()
        .unwrap_or("")
        .trim_start_matches("obj_");
    let (trajectory, object) = match class {
        "WellboreTrajectoryRepresentation" => (Ref::of_summary(summary, titled(store, summary)), None),
        "WellboreFrameRepresentation" | "SeismicWellboreFrameRepresentation" => {
            let (trajectory, _) = well_refs_of(store, reflect);
            let trajectory = trajectory?;
            let object =
                summary_of_ref(store, &trajectory).and_then(|found| store.get_object(&found.uri).ok());
            (trajectory, object)
        }
        _ => return None,
    };
    let reflect = match &object {
        Some(object) => object.as_reflect(),
        None if class == "WellboreTrajectoryRepresentation" => reflect,
        None => {
            return Some(crate::pieces::KnownPath {
                path: Err(format!(
                    "the trajectory `{}` the frame names is not in the package",
                    trajectory.title
                )),
                trajectory,
                md_uom: None,
                datum: None,
            });
        }
    };
    let interval = match reflect.field("MdInterval") {
        Some(FieldValue::Object(interval)) => Some(interval),
        _ => None,
    };
    let md_uom = interval
        .and_then(|interval| interval.field("Uom"))
        .and_then(|value| value.as_scalar())
        .and_then(|scalar| scalar.as_str().map(str::to_string));
    let datum = interval.and_then(|interval| match interval.field("Datum") {
        Some(FieldValue::Object(reference)) => {
            let found = followed(store, reference)?;
            Some(Ref::of_summary(found, titled(store, found)))
        }
        _ => None,
    });
    let path = energyml_geometry::well::path_of(reflect, store)
        .map_err(|why| format!("`{}`: {why}", trajectory.title));
    Some(crate::pieces::KnownPath {
        trajectory,
        path,
        md_uom,
        datum,
    })
}

/// The trajectory and the datum a frame names - their **references**, not
/// their values.
///
/// `wellhead_of` already walks this path to read the head; this returns the two
/// objects it crosses, because continuing a well requires rewriting **those**
/// objects and not copies whose uuid would be derived. A second trajectory that
/// named a derived datum would name a reference point that nothing else in the
/// package knows.
///
/// `(None, None)` for what is not a frame, and that is an answer: *this object
/// names no trajectory*.
fn well_refs_of(store: &EpcStore, reflect: &dyn energyml_core::value::Reflect) -> (Option<Ref>, Option<Ref>) {
    fn ref_at(store: &EpcStore, reflect: &dyn energyml_core::value::Reflect, field: &str) -> Option<Ref> {
        let energyml_core::value::FieldValue::Object(reference) = reflect.field(field)? else {
            return None;
        };
        let found = followed(store, reference)?;
        // The title is the one the **reference** carries: it is what a reader
        // sees, and the store's summary has none. Without it, the uri makes a
        // fallback name that at least names the object.
        let title = reference
            .field("Title")
            .and_then(|value| value.as_scalar())
            .and_then(|scalar| scalar.as_str().map(str::to_string))
            .unwrap_or_else(|| found.uri.clone());
        Some(Ref::of_summary(found, title))
    }

    let Some(trajectory) = ref_at(store, reflect, "Trajectory") else {
        return (None, None);
    };
    let datum = summary_of_ref(store, &trajectory)
        .cloned()
        .and_then(|summary| store.get_object(&summary.uri).ok())
        .and_then(|object| match object.as_reflect().field("MdInterval") {
            Some(energyml_core::value::FieldValue::Object(interval)) => ref_at(store, interval, "Datum"),
            _ => None,
        });
    (Some(trajectory), datum)
}

/// The `VerticalCrs` a compound system references, resolved in the package.
///
/// Two hops at most: the object names it (a `LocalEngineeringCompoundCrs`, a
/// `ReferencePointInACrs`), or it is a
/// `ReferencePointInALocalEngineeringCompoundCrs` whose `Crs` names the
/// compound that does (common TRG 2.3 §13.25 p. 371) - the datum form
/// a version translation writes, and the one chosen for a
/// datum under a compound. A `ProjectedCrs` names none, and `None` says so.
fn vertical_half(store: &EpcStore, reflect: &dyn energyml_core::value::Reflect) -> Option<Ref> {
    /// The reference a field holds.
    fn named<'a>(
        reflect: &'a dyn energyml_core::value::Reflect,
        field: &str,
    ) -> Option<&'a dyn energyml_core::value::Reflect> {
        match reflect.field(field)? {
            energyml_core::value::FieldValue::Object(reference) => Some(reference),
            _ => None,
        }
    }
    let found = match named(reflect, "VerticalCrs") {
        Some(reference) => followed(store, reference)?,
        None if reflect.type_info().type_key == energyml_domain::crs::DATUM_IN_COMPOUND => {
            let found = followed(store, named(reflect, "Crs")?)?;
            let compound = store.get_object(&found.uri).ok()?;
            followed(store, named(compound.as_reflect(), "VerticalCrs")?)?
        }
        None => return None,
    };
    Some(Ref::of_summary(found, titled(store, found)))
}

/// The title a reference to this object copies: its own `Citation/Title`, read
/// off the object - the store's summary carries none - and the uri only when
/// the object cannot be read or has no title.
fn titled(store: &EpcStore, found: &energyml_storage::resource::ObjectSummary) -> String {
    store
        .get_object(&found.uri)
        .ok()
        .and_then(|object| energyml_core::identity::title_of_reflect(object.as_reflect()).map(str::to_string))
        .unwrap_or_else(|| found.uri.clone())
}

/// A section view a package holds, as **its activity** says it: the activity,
/// and the polyline set it wrote (`None` when that object is not in the package
/// any more - still listed, and refused by name when reopened).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSection {
    /// The activity's uri.
    pub activity: String,
    /// Its title (`section view → <name>`).
    pub title: String,
    /// The section's name: the title of the polyline set it wrote, as the
    /// activity's reference states it.
    pub name: Option<String>,
    /// The polyline set's uri, when the package holds it.
    pub polyline: Option<String>,
    /// The parts of its drawings, as the activity names them: the main SVG
    /// (`svgPart`) first, then the further drawings (`drawingPart`) - read by
    /// their part name when reopened, with no index of the package.
    pub drawings: Vec<String>,
}

/// The section views stored in a package - listing the activities that point
/// to the right activity template is enough; there is no need to read all the
/// image files.
///
/// **Only the `Activity` objects are read**, picked from the listing by type;
/// each is kept when its `ActivityDescriptor` names `section-view`'s template
/// (its uuid, `provenance::template_uuid`) - never its title, which anybody may
/// edit. The polyline set is the `output` the activity names, the drawings the
/// parts its `svgPart` and `drawingPart` name; their bytes and metadata are read
/// only when it is reopened. On a package of sixty thousand objects this reads
/// its few activities, not its graph.
pub fn stored_sections(store: &EpcStore) -> Vec<StoredSection> {
    let Ok(template) = energyml_domain::provenance::template_uuid("section-view") else {
        return Vec::new();
    };
    let is_activity = |qualified: &str| {
        let local = qualified.rsplit('.').next().unwrap_or(qualified);
        local.strip_prefix("obj_").unwrap_or(local) == "Activity"
    };
    let field = |value: &serde_json::Value, name: &str| -> Option<serde_json::Value> {
        value
            .as_object()?
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };
    let text = |value: Option<serde_json::Value>| value.and_then(|v| v.as_str().map(str::to_string));
    let mut out = Vec::new();
    for summary in store
        .resources()
        .into_iter()
        .filter(|one| is_activity(&one.qualified_type))
    {
        let Ok(object) = store.get_object(&summary.uri) else {
            continue;
        };
        let json = energyml_core::json::write_value(object.as_reflect());
        let descriptor = field(&json, "ActivityDescriptor").and_then(|d| text(field(&d, "Uuid")));
        if !descriptor.is_some_and(|uuid| uuid.eq_ignore_ascii_case(template)) {
            continue;
        }
        let title = field(&json, "Citation")
            .and_then(|c| text(field(&c, "Title")))
            .unwrap_or_default();
        let parameters = field(&json, "Parameter")
            .and_then(|p| p.as_array().cloned())
            .unwrap_or_default();
        let wrote = parameters.iter().find_map(|p| {
            let reference = field(p, "DataObject")?;
            let kind = text(field(&reference, "QualifiedType"))
                .or_else(|| text(field(&reference, "ContentType")))?;
            let named_output = text(field(p, "Title")).is_some_and(|t| t == "output");
            (named_output && kind.contains("PolylineSetRepresentation")).then_some(reference)
        });
        let name = wrote.as_ref().and_then(|r| text(field(r, "Title")));
        // By the identity the activity's reference states - uuid, type and
        // `ObjectVersion` - so that a section drawn over a versioned copy names
        // the copy and not its original.
        let polyline = wrote.as_ref().and_then(|r| {
            let uuid = text(field(r, "Uuid"))?;
            let declared = text(field(r, "QualifiedType")).or_else(|| {
                let content = text(field(r, "ContentType"))?;
                energyml_core::identity::ObjectId::from_content_type(&content, &uuid)
                    .ok()
                    .map(|id| id.qualified_type())
            });
            let version = text(field(r, "ObjectVersion")).or_else(|| text(field(r, "VersionString")));
            let version = version
                .map(|one| one.trim().to_string())
                .filter(|one| !one.is_empty());
            store
                .resolve_reference(&uuid, declared.as_deref(), version.as_deref())
                .ok()
                .map(|one| one.uri.clone())
        });
        let parts = |named: &str| {
            parameters
                .iter()
                .filter(|p| text(field(p, "Title")).is_some_and(|t| t == named))
                .filter_map(|p| text(field(p, "Value")))
                .collect::<Vec<_>>()
        };
        let mut drawings = parts("svgPart");
        drawings.extend(parts("drawingPart"));
        out.push(StoredSection {
            activity: summary.uri,
            title,
            name,
            polyline,
            drawings,
        });
    }
    out.sort_by_key(|one| one.name.as_deref().unwrap_or(&one.title).to_lowercase());
    out
}
