//! `sigeol-units` — the BRGM SIGEOL export, as the first operation.
//!
//! This is the import that existed before the trait, reformulated on it and
//! not rewritten: `assemble` groups, `build` and `table` make the objects,
//! `crs` places the points, exactly as before. (The corpus tests that said
//! nothing moved, `tests/sigeol_corpus.rs`, read the exports where they sit
//! outside the repository and were removed on 29/09 — no test reads a file
//! outside the project.) What the trait adds
//! is the door: the same operation is reachable by `energyml create`, by the
//! front end and by a third-party application, and it says what it needs before
//! it is asked.
//!
//! # What it demands, fact by fact
//!
//! Written down as the model for every vocabulary to come
//!:
//!
//! | fact | required | without it |
//! |---|---|---|
//! | a `Point` per feature | yes | the feature is set aside and counted |
//! | the unit's name | yes | set aside and counted |
//! | the referential URL | no | the uuid is derived from the title — worse, and said |
//! | two **numeric** ages | to settle the side | no side is settled: the feature enters no surface |
//! | the queried **stage** | yes | refused — it decides top from base, and no GeoJSON states it |
//! | the CRS of the numbers | yes | 4326 by definition (RFC 7946) |
//! | a target CRS in metres | no | degrees are written as they stand, and a scene draws a needle |
//!
//! The stage is the one fact that is not in the file: it is read off the file
//! name when that follows `GeologicUnit_<Stage>`, and is a slot otherwise.

use energyml_domain::provenance::Parameter;

use crate::crs::{Local, Target, Vertical};
use crate::declare::{Epsg, Features, Inputs, Params, Suggestions};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Dropped, Error, Identity, Need, NewObject, Note,
    Numbers, Operation, Plan, Refusal, Slot,
};
use crate::pieces::Pieces;
use crate::{assemble, epc};

/// The operation.
pub struct SigeolUnits;

/// What `sigeol-units` is told.
#[derive(Debug, Params)]
pub struct SigeolParams {
    /// the stage the export was asked about — what decides top from base
    pub stage: String,
    /// the EPSG code the file's coordinates are in — 4326 by definition
    #[param(default = Epsg(4326))]
    pub source_epsg: Epsg,
    /// reproject into this EPSG system, in metres; left out, the degrees are written as they stand
    pub project: Option<Epsg>,
    /// the EPSG code of the vertical system, when known; left out, the package says unknown
    pub vertical: Option<Epsg>,
    /// the external file the coordinates go into
    #[param(default = String::from("data.h5"))]
    pub data_file: String,
}

/// What `sigeol-units` takes: the SIGEOL export — **several** collections
/// merged, as the plan always did.
#[derive(Debug, Inputs)]
pub struct SigeolInputs {
    /// the SIGEOL export, as a feature collection
    #[input(least = 1)]
    pub features: Vec<Features>,
}

impl Operation for SigeolUnits {
    type Params = SigeolParams;
    type Inputs = SigeolInputs;

    fn about(&self) -> About {
        About {
            name: "sigeol-units",
            describe: "the units of a BRGM SIGEOL GeologicUnit export: a feature, an interpretation and a point set per unit and side",
            category: Category::Import,
            makes: &[
                "resqml22.RockVolumeFeature",
                "resqml22.StratigraphicUnitInterpretation",
                "resqml22.PointSetRepresentation",
                "resqml22.DiscreteProperty",
                "eml23.ColumnBasedTable",
            ],
            recipe: "import-geojson",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        if let Some(found) = bag
            .features()
            .find_map(|(_, name, _)| crate::sigeol::stage_of_file_name(name))
        {
            slots.set("stage", serde_json::json!(found));
        }
    }

    /// **The content, not the kind**: a feature collection binds, and only a
    /// GeoSciML vocabulary inside it makes it an export this reads — the one
    /// named exception of `tests/operations.rs`.
    fn check(&self, inputs: &SigeolInputs) -> Result<(), Refusal> {
        let mut features = 0usize;
        let mut units = 0usize;
        for collection in &inputs.features {
            for feature in &collection.features {
                features += 1;
                if crate::sigeol::unit_of(&feature.properties).is_some() {
                    units += 1;
                }
            }
        }
        if units == 0 {
            return Err(Refusal::on(
                "features",
                format!(
                    "none of the {features} feature(s) carries a GeoSciML GeologicUnit this reader knows"
                ),
            ));
        }
        Ok(())
    }

    fn plan(&self, inputs: SigeolInputs, params: SigeolParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        if let Identity::Given(_) = cx.identity {
            return Err(Refusal::new(
                "refused",
                "`sigeol-units` names its objects from the producer's own identifiers, so a second run of \
                 the same file writes the same package; it does not take given uuids",
            ));
        }
        let mut collection = crate::geojson::Collection::default();
        let mut source = String::new();
        for one in &inputs.features {
            if source.is_empty() {
                source = one.file.clone();
            }
            collection.features.extend(one.features.iter().cloned());
        }
        let source_epsg = params.source_epsg.0;
        // **A code of degrees is no projection** — the refusal `support::frame_in`
        // makes of the same slot, for the same reason: `project: 4326` wrote a
        // `ProjectedCrs` whose numbers are degrees read as metres.
        if let Some(code) = params.project.map(|code| code.0)
            && crate::crs::is_geographic(code) == Some(true)
        {
            return Err(Refusal::on(
                "project",
                format!(
                    "`project` is EPSG:{code}, which measures degrees: a projection is into metres — name a projected system (EPSG:2154 for mainland France), or leave `project` out to keep the degrees"
                ),
            ));
        }
        let options = epc::Options {
            name: source.clone(),
            data_file: params.data_file,
            vertical: match params.vertical {
                Some(code) => Vertical::Epsg(code.0),
                None => Vertical::default(),
            },
            target: match params.project {
                Some(code) => Target::Projected(code.0),
                None => Target::Geographic(source_epsg),
            },
            source_epsg,
            creation: cx.when.creation.clone(),
            originator: cx.when.originator.clone(),
            source,
            stage: params.stage.clone(),
        };
        let assembled = assemble::plan(&collection, &params.stage);
        // **No group: say the stage asked for, and those the file carries.**
        // The earlier refusal — *“the plan holds no group”* — left the value
        // to type to be guessed; `top` and then `lutetien` were tried, where
        // the file writes `Lutétien`. The comparison does not fold accents
        // (`Unit::side_for`), so the names are given as written.
        if assembled.groups.is_empty() {
            let mut named: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for feature in &collection.features {
                if let Some(unit) = crate::sigeol::unit_of(&feature.properties) {
                    named.extend(
                        [unit.young.name, unit.old.name]
                            .into_iter()
                            .flatten()
                            .map(|one| one.trim().to_string()),
                    );
                }
            }
            let found = if named.is_empty() {
                "the file names no stage at all".to_string()
            } else {
                format!(
                    "the file's units name {}",
                    named
                        .iter()
                        .map(|one| format!("`{one}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            return Err(Refusal::on(
                "stage",
                format!(
                    "no unit of the file has `{}` at either end, so no point is the top or the base of it: {found}. \
                     Case is ignored, accents are not",
                    params.stage.trim()
                ),
            ));
        }
        Ok(plan_of(&assembled, &options)?)
    }

    fn reports(&self) -> Vec<Slot> {
        vec![
            Slot::required("source", Need::Text, "the file the features came from"),
            Slot::required("crsEpsg", Need::Epsg, "the EPSG code the points were written in"),
            Slot::optional(
                "verticalCrs",
                Need::Text,
                "the vertical system written, or unknown",
            ),
            Slot::required(
                "featureCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "features read",
            ),
            Slot::required(
                "pointCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "distinct points written",
            ),
            Slot::required(
                "duplicatesDropped",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "features on an already-written place",
            ),
        ]
    }
}

/// The plan of an assembled import — pure, and what `epc::write` applies.
///
/// The same objects, in the same order, as the writer built before the trait
/// existed: the frame's CRS objects (two in degrees; four over metres, the
/// compound a point set names among them), the table, then per group the
/// feature, its interpretation, its point set and its property, and the
/// coordinates.
pub fn plan_of(assembled: &assemble::Plan, options: &epc::Options) -> Result<Plan, Error> {
    let mut objects = Vec::new();
    let mut datasets = Vec::new();
    let mut says = Vec::new();
    let dropped: Vec<Dropped> = assembled
        .skipped()
        .into_iter()
        .map(|why| {
            let count = why.split(' ').next().and_then(|n| n.parse().ok()).unwrap_or(0);
            Dropped { count, why }
        })
        .collect();
    if assembled.groups.is_empty() {
        return Err(Error::new(
            "the plan holds no group: nothing to write".to_string(),
        ));
    }

    // **The frame of `support::frame_in`, by the same two functions**: the
    // identity compound over metres, the `Geographic2dCrs` over degrees — the
    // default here, and the named exception (a) of 24/09, said below. The
    // horizontal and vertical keep the uuids they always had; the plane and
    // the compound, over metres, are two objects more.
    let local = Local::identity();
    let names = crate::crs::names(
        &crate::operation::Identity::derived(),
        &options.target,
        &options.vertical,
        &local,
        &mut 0,
    )?;
    let crs = crate::crs::build(&options.target, &options.vertical, &local, &names, |title| {
        options.citation(title)
    })
    .map_err(Error::new)?;
    let crs_reference = crs.reference();
    if crs.local.is_none() {
        says.push(Note {
            key: "frame-degrees",
            detail: format!(
                "the point sets name `{}`, a Geographic2dCrs: their coordinates are degrees, and EML 2.3 has no compound over degrees (LocalEngineering2dCrs.OriginProjectedCrs is a ProjectedCrs, CRS.xsd l. 504) — so the vertical system is referenced by nothing. Give a projected target to write metres under a compound",
                crs_reference.title
            ),
        });
    }
    objects.extend(crs.new_objects());

    let rows = assemble::rows(assembled);
    let table_uuid = crate::naming::table(&format!("{}:{}", options.source, options.stage));
    let table = crate::table::build(
        &table_uuid,
        &rows,
        options.citation(&format!("{} — imported units", options.stage)),
    );
    let table_reference =
        crate::crs::reference_to(&table.uuid, "eml23.ColumnBasedTable", &table.citation.title);
    objects.push(NewObject {
        uuid: table.uuid.clone(),
        qualified_type: "eml23.ColumnBasedTable".to_string(),
        role: "table".to_string(),
        title: table.citation.title.clone(),
        json: energyml_core::json::write_value(&table),
    });

    let mut points_written = 0usize;
    for (group, row) in assembled.groups.iter().zip(&rows) {
        let key = group.unit.key(group.side).0;
        let path = format!(
            "/resqml22/{}/points",
            crate::naming::point_set(&key, &group.stage, group.side.label())
        );
        let array = crate::build::Array {
            file: options.data_file.clone(),
            path: path.clone(),
            mime: "application/x-hdf5".to_string(),
        };
        let made = crate::build::group(
            &crate::build::Group {
                unit: group.unit.clone(),
                side: group.side,
                stage: group.stage.clone(),
                points: group.points.clone(),
                features: group.features,
            },
            &array,
            &crs_reference,
            &|title| options.citation(title),
        );
        let title = made.representation.citation.title.clone();
        let representation = crate::crs::reference_to(
            &made.representation.uuid,
            "resqml22.PointSetRepresentation",
            &title,
        );
        let property = crate::table::property(
            &crate::naming::point_set(&key, &group.stage, &format!("{}-unit", group.side.label())),
            row.key,
            group.points.len(),
            &representation,
            &table_reference,
            options.citation(&format!("{title} — unit")),
        );

        let placed = match crate::crs::place(&group.points, &options.target, options.source_epsg) {
            Ok(placed) => placed,
            Err(why) => {
                says.push(crate::operation::Note {
                    key: "not-placed",
                    detail: format!("{title}: {why}"),
                });
                continue;
            }
        };
        objects.push(NewObject {
            uuid: made.feature.uuid.clone(),
            qualified_type: "resqml22.RockVolumeFeature".to_string(),
            role: "feature".to_string(),
            title: made.feature.citation.title.clone(),
            json: energyml_core::json::write_value(&made.feature),
        });
        objects.push(NewObject {
            uuid: made.interpretation.uuid.clone(),
            qualified_type: "resqml22.StratigraphicUnitInterpretation".to_string(),
            role: "interpretation".to_string(),
            title: made.interpretation.citation.title.clone(),
            json: energyml_core::json::write_value(&made.interpretation),
        });
        objects.push(NewObject {
            uuid: made.representation.uuid.clone(),
            qualified_type: "resqml22.PointSetRepresentation".to_string(),
            role: "representation".to_string(),
            title,
            json: energyml_core::json::write_value(&made.representation),
        });
        objects.push(NewObject {
            uuid: property.uuid.clone(),
            qualified_type: "resqml22.DiscreteProperty".to_string(),
            role: "property".to_string(),
            title: property.citation.title.clone(),
            json: energyml_core::json::write_value(&property),
        });
        points_written += group.points.len();
        datasets.push(Dataset {
            file: options.data_file.clone(),
            path,
            dimensions: vec![group.points.len() as i64, 3],
            values: Numbers::F64(placed.into_iter().flatten().collect()),
        });
    }

    let text = |title: &str, value: String| Parameter::Text {
        title: title.to_string(),
        value,
    };
    let number = |title: &str, value: f64| Parameter::Double {
        title: title.to_string(),
        value,
        uom: None,
    };
    let duplicates = assembled.features().saturating_sub(assembled.points());
    Ok(Plan {
        media: Vec::new(),
        blocked: None,
        objects,
        datasets,
        // **No shape for now**, and it is a named debt: this
        // import returns dozens of objects, one per unit, and the tree
        // of artefacts that matches them is exactly what a later design
        // will describe when an operation produces one. A flat `Made` per
        // group would be the list that design refuses.
        shapes: Vec::new(),
        activity: ActivityDraft {
            recipe: "import-geojson",
            title: format!("{} — {}", options.source, options.stage),
            seed: format!("{}:{}", options.source, options.stage),
            parameters: vec![
                text("source", options.source.clone()),
                text("stage", options.stage.clone()),
                number("sourceEpsg", options.source_epsg as f64),
                number("crsEpsg", options.target.epsg() as f64),
                text(
                    "verticalCrs",
                    match &options.vertical {
                        Vertical::Epsg(code) => format!("EPSG:{code}"),
                        Vertical::Unknown(_) => "unknown".to_string(),
                    },
                ),
                text("dataFile", options.data_file.clone()),
                number("featureCount", assembled.features() as f64),
                number("pointCount", points_written as f64),
                number("duplicatesDropped", duplicates as f64),
            ],
        },
        says,
        dropped,
    })
}
