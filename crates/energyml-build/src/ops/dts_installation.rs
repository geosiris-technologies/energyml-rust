//! `dts-installation` — a distributed temperature sensing fiber down a
//! wellbore: its optical path, the instrument box at its end, the installed
//! system that pairs them, and the header of a measurement.
//!
//! The reference model's DTS installation: a `DtsInstalledSystem` +
//! `FiberOpticalPath` (`FiberFacilityWell.Wellbore` 1..1) + `DtsMeasurement`
//! → `ChannelSet`, on one of its wells; and
//! `FiberFacilityWell.Wellbore` 1..1 → the WITSML wellbore of that well.
//!
//! | object | what it says |
//! |---|---|
//! | `DtsInstrumentBox` | the box at the fiber's end: `DtsInstalledSystem.InstrumentBox` is 1..1, so the installed system cannot be written without it |
//! | `FiberOpticalPath` | one segment the length of the fiber, a terminator, a facility mapping onto the wellbore |
//! | `DtsInstalledSystem` | the pairing in time of the path and the box |
//! | `DtsMeasurement` | the header of a survey, flagged empty: its traces are not written here |
//!
//! # The fourth object is the schema's, not an invention
//!
//! The reference model lists three objects. `DtsInstalledSystem.InstrumentBox` is a
//! `DataObjectReference` of multiplicity 1 (DtsInstalledSystem.xsd), and the
//! PRODML Technical Usage Guide v2.3, §19.5 p. 162: *« DTS uses an
//! intermediate object called DTS Installed System. This contains pointers
//! (data object references) to the Optical Path and DTS Instrument Box data
//! objects, which comprise the Installed System »*. A reference to nothing
//! would dangle, so the box is written — with an instrument named after the
//! installation and nothing else stated of it.
//!
//! # One segment, the length of the fiber
//!
//! Usage Guide v2.3, §19.3 p. 154: *« It is necessary to declare only those
//! components and segments that are of relevance to the particular
//! application or well design. If there is no interest in keeping track of
//! splices and connectors, you can easily declare this optical path to consist
//! of 1 fiber segment that happens to be the entire length of the optical path
//! »*. That is what is written, ended by a terminator `termination at cable` —
//! the single-ended case, *« where the optical path is single ended »* (same
//! page): nothing states a turnaround.
//!
//! The facility mapping (§19.3 p. 157, *« the translation between the actual
//! distance along the optical path as measured from the instrument box and the
//! actual distance of the measurement locations as measured from a reference
//! datum defined for the facility »*) is the simplest the schema allows: the
//! fiber starts at the wellbore's datum and runs down it, so an optical
//! distance **is** a measured depth, over the whole fiber. The plan says so.
//!
//! # A header, flagged empty
//!
//! `DtsMeasurement.MeasurementTrace` and `InterpretationLog` each reference a
//! `ChannelSet` that holds the values; none is written here — the dense
//! temperature profile of the well lives on its RESQML frame and in a WITSML
//! `Log` whose channel set is nested — which a reference **can** name
//! (`identity::nested_objects`, as `injection-test` does), but whose
//! traces are a depth profile, not the survey's. So the
//! header is written with `EmptySetFlag` true — *« Set to 'true' when the
//! measurement set is empty (only the header is provided) »* (the schema's own
//! documentation of the field) — and `BadSetFlag` false: nothing was measured
//! badly, nothing was measured.
//!
//! # Every object is synthetic, and sourced; identity from content
//!
//! As `prodml-doublet` does (reference-model rules on invented objects, ADR-0019). The box, the path and the
//! system come from the wellbore, the installation's date and the fiber's
//! length; the measurement from the system and its instant.

use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_prodml_2_3 as prodml;

use super::prodml_doublet::{titled, wellbore_because};
use super::prodml_writer::{Writer, instant};
use crate::declare::{Inputs, ObjectIn, Params};
use crate::operation::{About, ActivityDraft, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Ref;

/// The operation.
pub struct DtsInstallation;

/// Why a fiber wants a wellbore, after *« is a `Well`, and »*.
const NOT_A_WELL: &str = "a fiber is mapped onto a wellbore (FiberOpticalPath.xsd, `FiberFacilityWell.Wellbore` 1..1): give the wellbore";

/// Why a fiber wants a wellbore, after *« is a `…`, and »*.
const NOT_A_HOLE: &str =
    "a fiber is mapped onto a wellbore: a WITSML `Wellbore`, or the RESQML `WellboreFeature` of the hole";

// ---------------------------------------------------------------- the declaration

/// What `dts-installation` is told.
#[derive(Debug, Params)]
pub struct DtsInstallationParams {
    /// the title of the installation — the path, the box, the system and the measurement are named after it
    pub title: String,
    /// where the values come from — a document with its page or paragraph; a value stated as plausible says so (synthetic, and why)
    pub source: String,
    /// when the fiber was installed, a UTC date-time as 2025-01-01T00:00:00Z
    pub installed_from: String,
    /// when the survey was taken, a UTC date-time — on or after the installation
    pub measured_at: String,
    /// the length of the fiber, in m — from the wellbore's datum down
    #[param(min = 0.0, uom = "m")]
    pub fiber_length: f64,
}

/// What `dts-installation` takes.
#[derive(Debug, Inputs)]
pub struct DtsInstallationInputs {
    /// the wellbore the fiber runs down — a WITSML `Wellbore`, or the RESQML `WellboreFeature` of the hole
    #[input(types = "witsml21.Wellbore, resqml22.WellboreFeature")]
    pub wellbore: ObjectIn,
}

// ---------------------------------------------------------------- the operation

impl Operation for DtsInstallation {
    type Params = DtsInstallationParams;
    type Inputs = DtsInstallationInputs;

    fn about(&self) -> About {
        About {
            name: "dts-installation",
            describe: "a DTS fiber down a wellbore — its optical path mapped onto the hole, the instrument box, the installed system pairing them, the header of a survey — every value stated and sourced",
            category: Category::Synthetic,
            makes: &[
                "prodml23.DtsInstrumentBox",
                "prodml23.FiberOpticalPath",
                "prodml23.DtsInstalledSystem",
                "prodml23.DtsMeasurement",
            ],
            recipe: "dts-installation",
            represents: None,
        }
    }

    /// Whether the object given is a hole. The offer's refusal names the slot
    /// and the kind wanted, never the object's title nor its type: with one
    /// object slot, **any** object of a package is bound here, and its title
    /// is the producer's text — it may name a release, which would make an
    /// inaptitude read like a condition of writing (`no_unfitness_names_a_release`).
    /// The plan refuses the same object by its name.
    fn check(&self, inputs: &DtsInstallationInputs) -> Result<(), Refusal> {
        wellbore_because("wellbore", &inputs.wellbore, NOT_A_WELL, NOT_A_HOLE).map(|_| ()).map_err(|_| {
            Refusal::on("wellbore", "the object given is no wellbore: a fiber runs down a WITSML `Wellbore`, or the RESQML `WellboreFeature` of the hole")
        })
    }

    fn plan(
        &self,
        inputs: DtsInstallationInputs,
        params: DtsInstallationParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = titled("title", &params.title)?;
        let mut writer = Writer::new(cx, "dts-installation", &params.source)?;
        let (installed_text, installed) = instant("installedFrom", &params.installed_from)?;
        let (measured_text, measured) = instant("measuredAt", &params.measured_at)?;
        if measured < installed {
            return Err(Refusal::on(
                "measuredAt",
                format!(
                    "`{measured_text}` comes before the installation, `{installed_text}`: a fiber measures once it is in the hole"
                ),
            ));
        }
        let length = params.fiber_length;
        if !(length.is_finite() && length > 0.0) {
            return Err(Refusal::on(
                "fiberLength",
                format!("{length} m is not the length of a fiber: a finite number > 0"),
            ));
        }
        let hole = wellbore_because("wellbore", &inputs.wellbore, NOT_A_WELL, NOT_A_HOLE)?.clone();
        let dor = |known: &Ref| crate::crs::reference_of(known);
        let metres = |value: f64| eml::LengthMeasure {
            value,
            uom: eml::LengthUom::M,
        };

        // -- the uuids first: the system names the path and the box ----------------------
        let base = format!("{}:{installed_text}:{length}", hole.uuid.to_ascii_lowercase());
        let box_uuid = writer.uuid(&format!("instrument-box:{base}"))?;
        let path_uuid = writer.uuid(&format!("optical-path:{base}"))?;
        let system_uuid = writer.uuid(&format!("installed-system:{base}"))?;
        let measurement_uuid = writer.uuid(&format!("measurement:{system_uuid}:{measured_text}"))?;
        let box_title = format!("{title} — instrument box");
        let path_title = format!("{title} — optical path");
        let system_title = format!("{title} — installed system");
        let measurement_title = format!("{title} — survey {measured_text}");
        let box_ref = Ref::new(box_uuid.clone(), "prodml23.DtsInstrumentBox", box_title.clone());
        let path_ref = Ref::new(path_uuid.clone(), "prodml23.FiberOpticalPath", path_title.clone());
        let system_ref = Ref::new(
            system_uuid.clone(),
            "prodml23.DtsInstalledSystem",
            system_title.clone(),
        );

        // The box: an instrument named after the installation, nothing else stated.
        let instrument_box = prodml::DtsInstrumentBox {
            uuid: box_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(
                &box_title,
                &format!(
                    "the instrument box at the end of the invented fiber of `{}`",
                    hole.title
                ),
            )?,
            extension_name_value: vec![writer.synthetic("the instrument box of an invented fiber")],
            instrument: prodml::AnyInstrument::Instrument(prodml::Instrument {
                name: format!("{title} — instrument"),
                comment: Some(
                    "synthetic — no make, model or calibration is stated: none is written".to_string(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        };
        writer.push(
            &box_uuid,
            "prodml23.DtsInstrumentBox",
            "instrument-box",
            &box_title,
            &instrument_box,
        );

        // The optical path: one segment, a terminator, a mapping.
        let path = prodml::FiberOpticalPath {
            uuid: path_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(&path_title, &format!("an invented fiber of {length} m down `{}`, from its datum", hole.title))?,
            extension_name_value: vec![writer.synthetic("the optical path of an invented fiber")],
            inventory: prodml::FiberOpticalPathInventory {
                segment: vec![prodml::FiberOpticalPathSegment {
                    name: "segment 1".to_string(),
                    uid: "segment-1".to_string(),
                    fiber_length: metres(length),
                    comment: Some("one segment the length of the path (PRODML Technical Usage Guide v2.3, §19.3 p. 154)".to_string()),
                    ..Default::default()
                }],
                terminator: prodml::FiberTerminator {
                    name: "terminator".to_string(),
                    uid: "terminator-1".to_string(),
                    termination_type: prodml::TerminationKind::TerminationAtCable,
                    ..Default::default()
                },
                ..Default::default()
            },
            facility_mapping: vec![prodml::FiberFacilityMapping {
                time_start: installed_text.clone(),
                time_end: None,
                uid: "mapping-1".to_string(),
                comment: Some("synthetic — the fiber starts at the wellbore's datum: an optical distance is a measured depth".to_string()),
                fiber_facility_mapping_part: vec![prodml::FiberFacilityMappingPart {
                    optical_path_distance_start: metres(0.0),
                    optical_path_distance_end: metres(length),
                    facility_length_start: metres(0.0),
                    facility_length_end: metres(length),
                    fiber_facility: Some(prodml::AbstractFiberFacility::FiberFacilityWell(prodml::FiberFacilityWell {
                        name: hole.title.clone(),
                        wellbore: dor(&hole),
                        ..Default::default()
                    })),
                    uid: "mapping-part-1".to_string(),
                    ..Default::default()
                }],
            }],
            ..Default::default()
        };
        writer.push(
            &path_uuid,
            "prodml23.FiberOpticalPath",
            "optical-path",
            &path_title,
            &path,
        );

        // The installed system.
        let system = prodml::DtsInstalledSystem {
            uuid: system_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(
                &system_title,
                &format!(
                    "the invented fiber of `{}` paired with its instrument box from {installed_text}",
                    hole.title
                ),
            )?,
            extension_name_value: vec![writer.synthetic("the installed system of an invented fiber")],
            date_min: installed_text.clone(),
            optical_path_length: Some(metres(length)),
            optical_path: dor(&path_ref),
            instrument_box: dor(&box_ref),
            ..Default::default()
        };
        writer.push(
            &system_uuid,
            "prodml23.DtsInstalledSystem",
            "installed-system",
            &system_title,
            &system,
        );

        // The header of a survey.
        let measurement = prodml::DtsMeasurement {
            uuid: measurement_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(
                &measurement_title,
                &format!(
                    "the header of an invented survey of `{}` at {measured_text}; its traces are not written",
                    hole.title
                ),
            )?,
            extension_name_value: vec![writer.synthetic("the header of an invented DTS survey")],
            bad_set_flag: false,
            empty_set_flag: true,
            time_start: measured_text.clone(),
            installed_system: dor(&system_ref),
            measurement_configuration: prodml::OpticalPathConfiguration::SingleMinusEnded,
            facility_identifier: prodml::FacilityIdentifier {
                name: eml::NameStruct {
                    value: hole.title.clone(),
                    ..Default::default()
                },
                kind: Some(prodml::ReportingFacility::Wellbore.as_xml_str().to_string()),
                uid: "facility-1".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        writer.push(
            &measurement_uuid,
            "prodml23.DtsMeasurement",
            "measurement",
            &measurement_title,
            &measurement,
        );

        let says = vec![
            Note {
                key: "instrument-box",
                detail: "an instrument box is written though it is not a listed object of the reference model: DtsInstalledSystem.InstrumentBox is 1..1 (PRODML Technical Usage Guide v2.3, §19.5 p. 162)".to_string(),
            },
            Note {
                key: "one-segment",
                detail: format!("the optical path is one segment of {length} m ended at the cable — single ended (§19.3 p. 154): no splice, connector or turnaround is stated"),
            },
            Note {
                key: "mapping",
                detail: format!("the fiber is mapped onto `{}` from its datum: optical distance 0–{length} m is measured depth 0–{length} m", hole.title),
            },
            Note {
                key: "empty-survey",
                detail: "the survey is a header with EmptySetFlag true: no ChannelSet of traces is written here, so none is referenced".to_string(),
            },
            Note {
                key: "synthetic",
                detail: "every object written is invented: `synthetic = true` and the source in Citation/Description (ADR-0019)".to_string(),
            },
        ];
        let parameters = vec![
            Parameter::Double {
                title: "opticalPathLength".to_string(),
                value: length,
                uom: Some("m".to_string()),
            },
            Parameter::Object {
                title: "input".to_string(),
                reference: hole.dor(),
            },
        ];
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            activity: ActivityDraft {
                recipe: "dts-installation",
                title: format!("dts-installation → {title}"),
                seed: writer.activity_seed(),
                parameters,
            },
            objects: writer.objects,
            datasets: Vec::new(),
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }

    /// What the run wrote of the fiber.
    fn reports(&self) -> Vec<Slot> {
        vec![Slot::required(
            "opticalPathLength",
            Need::Number {
                uom: Some("m"),
                least: None,
                most: None,
            },
            "the length of the optical path the installed system states",
        )]
    }
}
