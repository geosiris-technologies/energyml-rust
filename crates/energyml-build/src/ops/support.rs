//! What the representation operations share: the frame the coordinates get,
//! the slots they all ask, the JSON they all write.
//!
//! # A frame is chosen or created, never guessed
//!
//! A representation needs a `LocalCrs`. Two honest ways to get one, and the
//! caller says which:
//!
//! - **`crs`** names an object of the package — as a `DataObjectReference`, or
//!   as a uuid that one of the inputs already resolved (`Known::crs`). That is
//!   the case of a shape read from the package or promoted from the scene,
//!   whose coordinates are already in that system and **must not move**;
//! - **`sourceEpsg`** (with `project` and `vertical`) creates the frame in the
//!   same plan and places the coordinates, exactly as the SIGEOL import does.
//!   That is the case of a GeoJSON, which is WGS 84 by definition.
//!
//! Neither is a default hidden in a signature: a shape without a frame is
//! refused by name, and a GeoJSON never silently lands in the package's own
//! system.
//!
//! # The frame is a compound, and a half is refused by its name
//!
//! RESQML's guide types what the XSD leaves open: *“From:
//! AbstractGeometry.LocalCrs — To: LocalEngineeringCompoundCrs”* (TRG v2.2,
//! §3.1, p. 22). So a frame **created** here is the compound
//! [`crate::crs::build`] writes over metres, and a frame **named** is judged by
//! [`crate::crs::frame_kind`] before anything points at it:
//!
//! - a compound, or a RESQML 2.0.1 local 3D system, is taken as it stands;
//! - a `ProjectedCrs` named directly — the BRGM's surfaces, our packages
//!   written before 24/09 — is taken, **with a note** (`frame-projected`):
//!   a deliberate decision, and the vertical it leaves unreferenced is said;
//! - a `Geographic2dCrs` is degrees, the exception (a), and said
//!   (`frame-degrees`) — created or named;
//! - a **half** — a `VerticalCrs`, a `LocalEngineering2dCrs`, a reference
//!   point — or anything that is no system is **refused by its name**, and the
//!   refusal is *carried*, not raised: the geometry does not depend on the
//!   frame, only its writing does (the rule a CRS of an older release
//!   already follows). Until 24/09 any reference was taken, and a geometry
//!   could name a `VerticalCrs` as its `LocalCrs`.

use serde_json::{Value, json};

use energyml_domain::provenance::Parameter;

use crate::crs::{FrameKind, Local, Target, Vertical};
use crate::geojson::{Feature, Position};
use crate::operation::{Error, Identity, Made, Need, NewObject, Note, Refusal, Slot, When};
use crate::pieces::{Known, Pieces, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

/// **A shape's faces, as the analysis takes them** — its patches concatenated in
/// their own order, each one's indices moved by the nodes before it, so value
/// *i* lands on node *i*: triangles, and four-cornered cells.
///
/// **One reading, for every operation that reads heights under a point**
/// (`thickness`, `project`). There were two, and both forgot the same case: a
/// 2D grid read as a lattice (`Topology::Grid`) has no listed faces, only a
/// shape — so both operations called a `Grid2dRepresentation` *a surface
/// without a face* while the scene drew it. Found on 14/09 by confronting the
/// front end's projected picking with `project` on the corpus. A lattice cell is a
/// quadrilateral here, read bilinearly: cutting it in two would be a decision.
pub(crate) struct Faces {
    pub points: Vec<f64>,
    pub triangles: Vec<u32>,
    pub quads: Vec<u32>,
}

impl Faces {
    pub(crate) fn of(geometry: &Geometry) -> Faces {
        let mut out = Faces {
            points: Vec::new(),
            triangles: Vec::new(),
            quads: Vec::new(),
        };
        for patch in &geometry.patches {
            let base = (out.points.len() / 3) as u32;
            out.points.extend(patch.points.iter().flatten().copied());
            match &patch.topology {
                Topology::Triangles(list) => out.triangles.extend(list.iter().flatten().map(|&i| i + base)),
                Topology::Quads(list) => out.quads.extend(list.iter().flatten().map(|&i| i + base)),
                Topology::Grid { fastest, slowest } => {
                    // Row-major, the fast axis first — the order the lattice
                    // evaluates its points in (`Geometry::drawable`). The ring
                    // goes round the cell: a, b along the row, then d, c back.
                    let fast = *fastest as u32;
                    for row in 0..slowest.saturating_sub(1) as u32 {
                        for column in 0..fast.saturating_sub(1) {
                            let a = base + row * fast + column;
                            let (b, c) = (a + 1, a + fast);
                            out.quads.extend([a, b, c + 1, c]);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Whether there is any face at all.
    pub(crate) fn any(&self) -> bool {
        !self.triangles.is_empty() || !self.quads.is_empty()
    }

    pub(crate) fn surface(&self) -> energyml_analysis::heights::Surface<'_> {
        energyml_analysis::heights::Surface {
            points: &self.points,
            triangles: &self.triangles,
            quads: &self.quads,
        }
    }
}

/// The frame the coordinates of a plan get.
pub struct Frame {
    /// The `LocalCrs` reference every geometry carries.
    pub crs: Value,
    /// The CRS objects created for it — empty when an existing one was named.
    pub objects: Vec<NewObject>,
    /// How foreign positions are placed into it: the target and the source
    /// EPSG. `None` when the coordinates are already in the frame.
    pub place: Option<(Target, i64)>,
    /// What the activity records of it.
    pub parameters: Vec<Parameter>,
    /// The vertical CRS of the frame, as a reference — created with it, or
    /// what the input knew of a frame of the package. `None` when unknown.
    pub vertical: Option<Value>,
    /// What would stop the **writing**, the geometry being computable anyway.
    ///
    /// Carried rather than returned as an error: see [`Plan::blocked`]. Two
    /// cases: a CRS of an older release than the objects these operations
    /// write, and a named reference that is no whole system — a half, a
    /// reference point.
    pub blocked: Option<Error>,
    /// What the frame is — `None` when there is none, and the plan is blocked.
    ///
    /// What a datum is written in depends on it (a reference point **in the
    /// compound**, decision (b) of 24/09; in a `ProjectedCrs` and a
    /// `VerticalCrs` otherwise), and an operation that writes vertical extents
    /// refuses the degrees by it.
    pub kind: Option<FrameKind>,
    /// What the plan must say of its frame: `frame-degrees` for the named
    /// exception, `frame-projected` for a `ProjectedCrs` taken as it stands.
    /// An operation puts these in its [`Plan::says`], after its own.
    pub says: Vec<Note>,
}

/// What a plan says of a frame in degrees — decision (a) of 24/09, **an
/// exception named rather than a refusal**: there is no compound over a
/// `Geographic2dCrs` in EML 2.3, since `LocalEngineering2dCrs.OriginProjectedCrs`
/// is a `ProjectedCrs` (`CRS.xsd` l. 504).
fn degrees_note(title: &str) -> Note {
    Note {
        key: "frame-degrees",
        detail: format!(
            "the geometry names `{title}`, a Geographic2dCrs: its coordinates are degrees, and EML 2.3 has no compound over degrees (LocalEngineering2dCrs.OriginProjectedCrs is a ProjectedCrs, CRS.xsd l. 504) — so the vertical system is referenced by nothing, and RESQML TRG 2.2 §3.1 p. 22, which associates a geometry with a LocalEngineeringCompoundCrs, is not followed. Give `project` to write metres under a compound"
        ),
    }
}

/// The slots every representation asks — with what the inputs suggest.
///
/// `represents` is what the operation writes when no interpretation is named —
/// see [`crate::operation::AnyOperation::represents`]. It is passed rather than
/// read back because these slots are built before an operation is in hand, and
/// a slot that can create must **say so**: an optional field reads *you may do
/// without*, not *I make it for you*.
///
/// # The frame's two halves are alternatives, and they now say it
///
/// `crs` names a system the coordinates are already in; `sourceEpsg` (with
/// `project` and `vertical`) builds one and places them. Answering either makes
/// the other meaningless, and until 10/09 a form showed all four at once —
/// the form asked for a CRS **and** EPSG codes. [`Applies`] carries the
/// exclusion: the form hides what cannot act, and `settle` refuses a caller who
/// sends both.
pub fn common_slots(pieces: &Pieces, represents: Option<(&'static str, &'static str)>) -> Vec<Slot> {
    // **The slots are the typed blocks'** (`ops/written.rs`) — one
    // declaration, whether the operation is typed or not; what this adds is
    // the suggestions the bag makes.
    use crate::declare::{Params, Suggestions};
    let mut slots = match represents {
        None => super::written::Written::slots(),
        Some(_) => super::written::WrittenImposed::slots(),
    };
    let mut suggestions = Suggestions::new(&mut slots);
    super::written::suggest_written(pieces, represents, &mut suggestions);
    debug_assert!(
        suggestions.missed().is_empty(),
        "shared slots missing: {:?}",
        suggestions.missed()
    );
    slots
}

/// The typed blocks — see `ops/written.rs`.
pub use super::written::{Written, WrittenImposed};

/// The feature and its interpretation, made together — or the reference that has been
/// named.
///
/// # What it writes, and what it does not write yet
///
/// It carries the pair of **the four generic representations**: the two
/// objects, the link between them, and the kind when the type asks for one. The type
/// comes from an operation that imposes it (`AnyOperation::represents`) or from a
/// person who chooses it (`featureType` / `interpretationType`).
///
/// **`well` and `boundary` keep their own**, and it has to be said rather than
/// pretending otherwise: their objects carry fields that this one does not write
/// — `Domain` and `IsDrilled` on a well interpretation, a
/// `CulturalFeatureKind` that `boundary` takes from a closed choice and not from a
/// free text. Folding them in here means deciding where those fields live, which
/// is work to be done and not a line to write; meanwhile, two
/// copies remain, known and named.
///
/// Nothing is written when `interpretation` is answered: a second one is not created
/// to be put beside the one that has just been named.
pub struct Represented {
    /// What the representation puts in `RepresentedObject` — `None` when
    /// nothing was named or chosen, and the object then comes out without.
    pub reference: Option<Value>,
    /// The objects made — empty when an interpretation was named.
    pub objects: Vec<NewObject>,
}

/// The same, from the typed blocks — the name-reading [`represented_by`] is
/// its wrapper for the operations not yet typed. `new_feature` is `None` for
/// an operation whose block has no feature to choose.
#[allow(clippy::too_many_arguments)]
pub fn represented_in(
    represented: &super::written::RepresentedParams,
    new_feature: Option<&super::written::NewFeatureParams>,
    imposed: Option<(&str, &str)>,
    identity: &Identity,
    when: &When,
    seed: &str,
    title: &str,
    rank: &mut usize,
) -> Result<Represented, Error> {
    if let Some(value) = represented.interpretation.as_ref().map(|one| &one.0) {
        if !(value.is_object() && value["Uuid"].is_string()) {
            return Err(Refusal::about(
                "value",
                "interpretation",
                format!("`interpretation`: expected a DataObjectReference (Uuid, QualifiedType, Title), got {value}"),
            )
            .into());
        }
        return Ok(Represented {
            reference: Some(value.clone()),
            objects: Vec::new(),
        });
    }
    let pair = match imposed {
        Some((feature, interpreted)) => Some((feature.to_string(), interpreted.to_string())),
        None => new_feature.and_then(|chosen| {
            chosen
                .feature_type
                .as_ref()
                .map(|feature| (feature.0.clone(), chosen.interpretation_type.0.clone()))
        }),
    };
    let Some((feature_type, interpretation_type)) = pair else {
        return Ok(Represented {
            reference: None,
            objects: Vec::new(),
        });
    };

    let feature_uuid = next_uuid(identity, &format!("feature:{seed}"), rank)?;
    let feature_title = format!("{title} — feature");
    let mut feature = json!({
        "$type": feature_type,
        "uuid": feature_uuid,
        "schemaVersion": "2.2",
        "Citation": citation(when, &feature_title),
        // Every RESQML feature carries it, and *false* is the only honest
        // answer: this one was made here, and a well-known feature is one a
        // published catalogue names.
        "IsWellKnown": false,
    });
    // **What a type asks for in addition and that this one can give.** A
    // `CulturalFeature` carries a kind; the others have none, and setting it
    // all the same would make the object carry a field its schema does not
    // declare. The field is written only for a type whose name announces it, and the
    // plan says so when a kind was requested for a type that has none.
    let wants_kind = feature_type.ends_with("CulturalFeature");
    if wants_kind {
        let kind = new_feature
            .and_then(|chosen| chosen.feature_kind.as_deref())
            .unwrap_or("model frontier");
        feature["CulturalFeatureKind"] = json!(kind);
    }

    let interpretation_uuid = next_uuid(identity, &format!("interpretation:{seed}"), rank)?;
    let interpretation_title = format!("{title} — interpretation");
    let interpretation = json!({
        "$type": interpretation_type,
        "uuid": interpretation_uuid,
        "schemaVersion": "2.2",
        "Citation": citation(when, &interpretation_title),
        "InterpretedFeature": {
            "$type": "eml23.DataObjectReference",
            "Uuid": feature_uuid,
            "QualifiedType": feature_type,
            "Title": feature_title,
        },
    });

    Ok(Represented {
        reference: Some(json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": interpretation_uuid,
            "QualifiedType": interpretation_type,
            "Title": interpretation_title,
        })),
        objects: vec![
            NewObject {
                uuid: feature_uuid,
                qualified_type: feature_type,
                role: "feature".to_string(),
                title: feature_title,
                json: feature,
            },
            NewObject {
                uuid: interpretation_uuid,
                qualified_type: interpretation_type,
                role: "interpretation".to_string(),
                title: interpretation_title,
                json: interpretation,
            },
        ],
    })
}

/// A title read off the first input that has a name.
pub(crate) fn suggested_title(pieces: &Pieces) -> Option<String> {
    pieces.iter().find_map(|(name, source)| match source {
        Source::Features { name: file, .. } => Some(file.clone()),
        Source::Object(known) => known.reference.as_ref().map(|r| r.title.clone()),
        Source::Shape { .. } | Source::Lattice(_) => Some(name.to_string()),
        _ => None,
    })
}

/// The frame, from the typed block — chosen in the package or created here.
///
/// `known` is what the inputs knew of their objects: it resolves a `crs`
/// given as a bare uuid, and gives the vertical half of a frame of the
/// package. `rank` is where the created objects start in the plan's
/// numbering, and it is advanced past them.
pub fn frame_in(
    known: &[&Known],
    params: &super::written::FrameParams,
    identity: &Identity,
    when: &When,
    rank: &mut usize,
) -> Result<Frame, Error> {
    if let Some(value) = params.crs.as_ref().map(|crs| &crs.0) {
        let crs = match value {
            Value::Object(_) if value["Uuid"].is_string() => value.clone(),
            Value::String(uuid) => {
                let known = known
                    .iter()
                    .filter_map(|known| known.crs.as_ref())
                    .find(|r| r.uuid.eq_ignore_ascii_case(uuid))
                    .ok_or_else(|| {
                        Error::from(Refusal::about(
                            "value",
                            "crs",
                            format!("`crs` names {uuid}, which no input resolved: give a DataObjectReference (Uuid, QualifiedType, Title)"),
                        ))
                    })?;
                known.dor()
            }
            other => {
                return Err(Refusal::about(
                    "value",
                    "crs",
                    format!("`crs`: expected a DataObjectReference or a uuid, got {other}"),
                )
                .into());
            }
        };
        // **What it is decides what it may carry**, and the release too: these
        // operations write RESQML 2.2, and a 2.0.1 CRS would need an
        // `EpcExternalPartReference` proxy and its rels for every array, which
        // is not written yet. Refused by name — the CRS is how the release was
        // chosen — rather than producing a part half of one release. And a
        // half of a system — a `VerticalCrs`, a `LocalEngineering2dCrs` — or a
        // reference point is refused by its name too: a geometry naming one
        // reads back with no horizontal, or no vertical, and looks whole.
        // **Carried, not raised.** The computation does not depend on it: this CRS only enters
        // the `LocalCrs` of the object written. Refusing here would refuse to
        // *compute* for a reason that only concerns the *writing* — the
        // defect seen on `outline` and a real dataset, where the operation returned
        // five perfectly good boundary rings anyway.
        let qualified = crs["QualifiedType"].as_str().unwrap_or_default().to_string();
        let title = crs["Title"].as_str().unwrap_or_default().to_string();
        let kind = crate::crs::frame_kind(&qualified);
        let mut says = Vec::new();
        let blocked = match &kind {
            Err(why) => Some(Error::from(Refusal::about(
                "value",
                "crs",
                format!("`crs` names no whole system: {why}"),
            ))),
            Ok(FrameKind::Legacy) => Some(Error::from(Refusal::about(
                "value",
                "crs",
                format!(
                    "`crs` is `{qualified}`, a RESQML 2.0.1 system, and this operation writes 2.2 objects: choose an eml 2.3 CRS, or translate the package first"
                ),
            ))),
            Ok(FrameKind::Projected) => {
                // Decision (c) of 24/09: accepted, and said. The BRGM's own
                // surfaces point at their `ProjectedCrs`; refusing them would
                // make every operation on them a dead end.
                says.push(Note {
                    key: "frame-projected",
                    detail: format!(
                        "the geometry names `{title}`, a ProjectedCrs, directly — accepted as it stands, though RESQML TRG 2.2 §3.1 p. 22 associates a geometry with a LocalEngineeringCompoundCrs: the vertical system is then referenced by nothing, and a reader takes the third ordinate as an elevation in the frame's unit"
                    ),
                });
                None
            }
            Ok(FrameKind::Geographic) => {
                says.push(degrees_note(&title));
                None
            }
            Ok(FrameKind::Compound) => None,
        };
        // The vertical half, when an input knew it for this very frame.
        let uuid = crs["Uuid"].as_str().unwrap_or_default().to_string();
        let vertical = known
            .iter()
            .filter(|known| {
                known
                    .crs
                    .as_ref()
                    .is_some_and(|c| c.uuid.eq_ignore_ascii_case(&uuid))
            })
            .find_map(|known| known.vertical_crs.as_ref().map(|r| r.dor()));
        return Ok(Frame {
            parameters: vec![Parameter::Object {
                title: "input".to_string(),
                reference: crs.clone(),
            }],
            crs,
            objects: Vec::new(),
            place: None,
            vertical,
            blocked,
            kind: kind.ok(),
            says,
        });
    }
    // **No frame at all: carried, not raised** (made whole
    // by the rule above). The geometry does not depend on it — it is computed in the
    // frame the inputs came in — and only the writing does. A placeholder
    // reference stands where the `LocalCrs` would go, `blocked` says why the
    // objects cannot be applied, and a plan asked for its geometry comes back
    // with its shapes intact.
    let Some(source_epsg) = params.source_epsg.map(|code| code.0) else {
        return Ok(Frame {
            crs: json!({
                "$type": "eml23.DataObjectReference",
                "Uuid": "00000000-0000-0000-0000-000000000000",
                "QualifiedType": "eml23.LocalEngineeringCompoundCrs",
                "Title": "no frame given",
            }),
            objects: Vec::new(),
            place: None,
            parameters: Vec::new(),
            vertical: None,
            blocked: Some(Error::new(
                "a frame is needed to write: `crs` (an object of the package the coordinates are already in) or `sourceEpsg` (the CRS objects are created here)"
                    .to_string(),
            )),
            kind: None,
            says: Vec::new(),
        });
    };
    let target = match params.project.map(|code| code.0) {
        Some(code) => {
            // **A code of degrees is no projection** — the defect of a
            // degree code under a projection, here by the other door: `project: 4326` wrote a
            // `ProjectedCrs` whose numbers are degrees read as metres. *I do
            // not know* (`None`, a code this build lacks) is not *no*, and the
            // reprojection refuses it by name further on.
            if crate::crs::is_geographic(code) == Some(true) {
                return Err(Refusal::about(
                    "value",
                    "project",
                    format!("`project` is EPSG:{code}, which measures degrees: a projection is into metres — name a projected system (EPSG:2154 for mainland France), or leave `project` out to keep the degrees"),
                )
                .into());
            }
            Target::Projected(code)
        }
        // **Without a target, the target is the source** — and what it is
        // decides the form. Until 24/09 a source in metres left without
        // `project` was written as a `Geographic2dCrs` carrying a projected
        // code, its numbers then read as degrees: the reverse of that. The rule
        // `wells-from-table` already follows (*without a target, the target is the
        // source*), taken here for every writer. A code this build does not
        // know keeps the old reading — *I do not know* is not *metres*.
        None if crate::crs::is_geographic(source_epsg) == Some(false) => Target::Projected(source_epsg),
        None => Target::Geographic(source_epsg),
    };
    let vertical = match params.vertical.map(|code| code.0) {
        Some(code) => Vertical::Epsg(code),
        None => Vertical::default(),
    };
    let vertical_key = vertical.key();
    // **The identity**: what an operation writes under it is what it wrote
    // under the `ProjectedCrs` before — an easting, a northing, an elevation.
    // An origin, an azimuth or another axis is the `crs` operation's to ask.
    let local = Local::identity();
    let names = crate::crs::names(identity, &target, &vertical, &local, rank)?;
    let built = crate::crs::build(&target, &vertical, &local, &names, |title| {
        energyml_eml_2_3::Citation {
            title: title.to_string(),
            originator: when.originator.clone(),
            creation: when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        }
    })
    .map_err(Error::new)?;
    let reference = built.reference();
    let crs = energyml_core::json::write_value(&reference);
    let objects = built.new_objects();
    let vertical = Some(energyml_core::json::write_value(&built.vertical_reference()));
    let (kind, says) = match &target {
        Target::Projected(_) => (FrameKind::Compound, Vec::new()),
        Target::Geographic(_) => (FrameKind::Geographic, vec![degrees_note(&reference.title)]),
    };
    Ok(Frame {
        crs,
        objects,
        vertical,
        // Nothing blocks: the frame is created here, hence in the release that
        // the operation writes.
        blocked: None,
        kind: Some(kind),
        says,
        parameters: vec![
            Parameter::Double {
                title: "sourceEpsg".to_string(),
                value: source_epsg as f64,
                uom: None,
            },
            Parameter::Double {
                title: "crsEpsg".to_string(),
                value: target.epsg() as f64,
                uom: None,
            },
            Parameter::Text {
                title: "verticalCrs".to_string(),
                value: vertical_key,
            },
        ],
        place: Some((target, source_epsg)),
    })
}

/// Foreign positions, placed into the frame — or refused when the frame is
/// one of the package, whose numbers a GeoJSON's cannot be assumed to be in.
pub fn placed(positions: &[Position], frame: &Frame) -> Result<Vec<[f64; 3]>, Error> {
    match &frame.place {
        Some((target, from)) => crate::crs::place(positions, target, *from).map_err(Error::new),
        None => Err(Refusal::about(
            "value",
            "crs",
            "a feature collection's coordinates are in their own system: give `sourceEpsg` (and `project`) rather than a `crs` of the package",
        )
        .into()),
    }
}

/// A shape's nodes, in the frame being written.
///
/// A shape carries numbers and no system of its own, so where they are is
/// read from where it came from. The defect of 29/09 is what this closes:
/// `create pointset --from shape=… --set sourceEpsg=3949 --set project=2154`
/// wrote CC49 numbers under a Lambert-93 label, a thousand kilometres from
/// their place, and the package validated clean — only a feature collection
/// was placed.
///
/// - **a frame of the package named** (`crs`): the shape is taken as already
///   in it, as it always was;
/// - **a frame created here** (`sourceEpsg`, `project`): a shape read from a
///   file is in `sourceEpsg`, and is placed as a feature collection's
///   positions are, through `energyml-proj`;
/// - **but a shape that is an object of the package** is in that object's own
///   frame, not in `sourceEpsg`: reprojecting it from a code it is not in
///   would be the same defect by another door, so it is refused by name.
///
/// Both the placing and the refusal apply only when numbers **move** — a
/// target other than the source. `sourceEpsg` and `project` naming the same
/// code (what `wellheads` asks, its collars already in the wells' own
/// system) moves nothing, and a shape of the package passes as it always did.
pub fn shape_placed(
    name: &str,
    points: &[[f64; 3]],
    known: Option<&Known>,
    frame: &Frame,
) -> Result<Vec<[f64; 3]>, Error> {
    let Some((target, from)) = &frame.place else {
        return Ok(points.to_vec());
    };
    let moves = matches!(target, Target::Projected(to) if to != from);
    if !moves {
        return Ok(points.to_vec());
    }
    if let Some(crs) = known.and_then(|known| known.crs.as_ref()) {
        return Err(Refusal::about(
            "value",
            "sourceEpsg",
            format!(
                "`{name}` is an object of the package: its coordinates are in its own frame `{}`, not in EPSG:{from} — name that frame as `crs` to write in it; `sourceEpsg` says where the numbers of a file or a feature collection are",
                crs.title
            ),
        )
        .into());
    }
    let positions: Vec<Position> = points
        .iter()
        .map(|p| Position {
            lon: p[0],
            lat: p[1],
            z: Some(p[2]),
        })
        .collect();
    crate::crs::place(&positions, target, *from).map_err(Error::new)
}

/// A shape's geometry, in the frame being written — [`shape_placed`] patch
/// by patch, the topology untouched: a reprojection moves nodes, it neither
/// adds nor removes one, so every index still points where it pointed.
pub fn geometry_placed(
    name: &str,
    geometry: &Geometry,
    known: Option<&Known>,
    frame: &Frame,
) -> Result<Geometry, Error> {
    let mut placed = geometry.clone();
    if frame.place.is_some() {
        for patch in &mut placed.patches {
            patch.points = shape_placed(name, &patch.points, known, frame)?;
        }
    }
    Ok(placed)
}

/// A citation for an object of the plan.
pub fn citation(when: &When, title: &str) -> Value {
    json!({
        "$type": "eml23.Citation",
        "Title": title,
        "Originator": when.originator,
        "Format": energyml_core::CITATION_FORMAT,
        "Creation": when.creation,
    })
}

/// The `PointGeometry` of a patch: its frame and its points in the file.
pub fn point_geometry(crs: &Value, file: &str, path: &str, count: usize) -> Value {
    json!({
        "$type": "resqml22.PointGeometry",
        "LocalCrs": crs,
        "Points": {
            "$type": "resqml22.Point3dExternalArray",
            "Coordinates": external(file, path, &[count as i64, 3]),
        },
    })
}

/// An `ExternalDataArray` of that shape at that path.
pub fn external(file: &str, path: &str, count: &[i64]) -> Value {
    json!({
        "$type": "eml23.ExternalDataArray",
        "ExternalDataArrayPart": [{
            "$type": "eml23.ExternalDataArrayPart",
            "Count": count,
            "StartIndex": count.iter().map(|_| 0).collect::<Vec<i64>>(),
            "MimeType": "application/x-hdf5",
            "PathInExternalFile": path,
            "URI": file,
        }],
    })
}

/// The point count of a feature collection's parts, for a fitness message.
pub fn positions_of(features: &[Feature]) -> Vec<Position> {
    features.iter().flat_map(|f| f.positions()).collect()
}

/// The name of the first input, for a seed.
pub fn seed_of(pieces: &Pieces, title: &str) -> String {
    let first = pieces
        .iter()
        .next()
        .map(|(name, _)| name.to_string())
        .unwrap_or_default();
    format!("{first}:{title}")
}

/// What the frame reports in the activity, ahead of an operation's own counts:
/// the EPSG code the points were written in and the vertical system, when the
/// frame was created here — absent when one of the package was named.
pub fn frame_reports(mut own: Vec<Slot>) -> Vec<Slot> {
    let mut out = vec![
        Slot::optional(
            "crsEpsg",
            Need::Epsg,
            "the EPSG code the points were written in, when the CRS was created here",
        ),
        Slot::optional(
            "verticalCrs",
            Need::Text,
            "the vertical system written, or unknown — when the CRS was created here",
        ),
    ];
    out.append(&mut own);
    out
}

/// A coordinate or a depth **as it enters a seed**: to the millimetre, never
/// to the last bit.
///
/// A seed that writes a computed float writes whatever the platform's libm
/// left in its last bit — a head reprojected from WGS 84, a ground read on a
/// DTM interpolated from such heads — and the uuid derived from it then
/// differs between two machines for the same object. The CI found it on
/// 04/10: Lutetia replayed on Linux wrote its wells under other uuids than on
/// Windows, and the stored section drawing, which names them, was refused.
///
/// Rounded to the millimetre, a value typed with three decimals or fewer is
/// **the same `f64`** as before (`34.27 * 1000` rounds to `34270`, and
/// `34270 / 1000` is the double nearest `34.27`), so its seed — whatever the
/// caller formats it with — and its uuid do not move; only computed values
/// change, and those were not stable anyway. `-0.0` is written `0.0`: a
/// rounding to zero must not depend on the side it came from.
pub fn seed_mm(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0 + 0.0
}

/// The uuid of the next object of the plan.
pub fn next_uuid(identity: &Identity, seed: &str, rank: &mut usize) -> Result<String, Error> {
    let uuid = identity.uuid(seed, *rank)?;
    *rank += 1;
    Ok(uuid)
}

/// The root of an artefact tree, for an operation that returns only one shape.
///
/// Today's thirteen operations are in this case: they compute a
/// geometry and make an object of it. A `fence` will return several, under a
/// root — see [`crate::operation::Made`].
pub fn made_root(title: &str, uuid: &str, geometry: Geometry) -> Made {
    Made {
        id: "0".to_string(),
        parent: None,
        title: title.to_string(),
        geometry,
        object: Some(uuid.to_string()),
    }
}

/// A shape of a single patch, what an operation almost always computes.
pub fn one_patch(kind: &'static str, points: Vec<[f64; 3]>, topology: Topology) -> Geometry {
    Geometry {
        kind,
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology,
            closed: Vec::new(),
        }],
    }
}

/// The points of a shape, flat — what a `Dataset` writes.
///
/// Read **from the shape** rather than kept beside it: without that the points
/// would be in the plan twice, and a plan of a million points would pay
/// twice. The shape is the source, the array is a view of it.
pub fn flat_points(geometry: &Geometry) -> Vec<f64> {
    geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().flatten().copied())
        .collect()
}
