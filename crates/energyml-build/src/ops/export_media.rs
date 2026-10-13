//! `export-media` - one representation of the package, written by an exporter
//! of `energyml-export`, and put **in the package** beside it.
//!
//! In the reference model: an OBJ export of a surface, a GeoJSON export of a
//! point set in WGS 84, a VTK XML export of a grid, each related to the object it shows. The part and its two
//! relationships are [`super::media`]'s - `Plan::media`, written by `apply`
//! with `destinationMedia` from the object and `sourceMedia` back (EPC §3.4.2
//! pp. 15-16). What this operation adds is the **bytes**, and they come from
//! the export registry and nowhere else: a second writer of OBJ here would be
//! a second place for the same mistakes, and the file in the package would
//! stop being what `energyml export` writes.
//!
//! # What is decided here
//!
//! - **the content type is the exporter's own `mime()`** - content types come
//!   from the exporter, not from a second table.
//!   There is no `contentType` setting to disagree with it;
//! - **the object is rebuilt from the bag**, never from a store: the shape
//!   `read::pieces_of_object` reads (local, as a writing plan reads it) and
//!   its companion - the reference, the interpretation, and the frame that
//!   places it (`Known::placement`). That is everything an exporter reads of
//!   a [`SceneObject`] but two things, and both are said rather than filled:
//!   the **feature** above the interpretation is not in the bag, so the file
//!   names the object and its interpretation and no feature (`identity`); and
//!   no property rides along - `attributes` stay empty, as in an `energyml
//!   export` without `--properties`;
//! - **a GeoJSON is reprojected by `energyml-proj`**, the one `Reprojector`
//!   the workspace ships (pure Rust, wasm included) - RFC 7946 fixes WGS 84,
//!   and the exporter refuses without a backend rather than writing eastings
//!   as longitudes;
//! - **the default part name is the library's**: `media/export/<title>.<ext>`,
//!   the title folded to a file name (`H3 - top of the Yprésien` →
//!   `h3-top-of-the-ypresien`); `path` overrides it, checked by the function
//!   the store applies (`energyml_storage::epc::media::check_part_name`), so
//!   what `apply` would refuse is refused here, in the same words.
//!
//! # What is refused, by name
//!
//! - a shape that is **no object of the package** (an artefact, a mesh file):
//!   the part is related to what it shows, and an export with no uuid cannot
//!   be traced back - the reason exports carry an identity at all
//!   (`energyml-export`, *Identity travels with the geometry*);
//! - a shape whose frame is **named but not given** - read placed, or a frame
//!   that could not be read: the exporter places the numbers itself, and
//!   placing them twice, or not at all, writes a plausible file in the wrong
//!   place;
//! - a format the registry does not hold, listing what it holds;
//! - an object the exporter declines - no EPSG code for a GeoJSON, a vertical
//!   axis in travel time (an export states no equivalence here), no point -
//!   with the reason the exporter gives when it writes.
//!
//! No object is made, so no uuid is derived. The activity records the part,
//! its type, its size and the object exported (`input`).

use energyml_domain::provenance::Parameter;
use energyml_export::{Exporter, Exporters, Options};
use energyml_geometry::{Related, SceneObject};
use energyml_storage::epc::media::{check_media_type, check_part_name};

use crate::declare::{Inputs, Params, Shape};
use crate::operation::{
    About, ActivityDraft, Category, Context, Media as Part, Need, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::{Known, Ref};

/// The operation.
pub struct ExportMedia;

/// What `export-media` is told.
#[derive(Debug, Params)]
pub struct ExportMediaParams {
    /// the exporter to write with - a name the export registry holds, as `energyml export --list` prints them (`obj`, `geojson`, `vtu`…)
    pub format: String,
    /// the part name in the package - left out, `media/export/` and the object's title with the exporter's extension (EPC §3.2: "a folder named media")
    pub path: Option<String>,
}

/// What `export-media` takes.
#[derive(Debug, Inputs)]
pub struct ExportMediaInputs {
    /// the representation to export - an object of the package, which the file is then a media representation of
    #[input(
        types = "resqml22.TriangulatedSetRepresentation, resqml22.PolylineSetRepresentation, resqml22.PointSetRepresentation, resqml22.Grid2dRepresentation, resqml22.IjkGridRepresentation, resqml22.UnstructuredGridRepresentation, resqml22.WellboreTrajectoryRepresentation"
    )]
    pub shape: Shape,
}

/// The object of the package the shape is, and what is known of it - or the
/// refusal that says it is none.
fn object_of(shape: &Shape) -> Result<(&Ref, &Known), Refusal> {
    match shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref().map(|reference| (reference, known)))
    {
        Some(found) => Ok(found),
        None => Err(Refusal::on(
            "shape",
            format!(
                "`{}` is no object of the package: an export is related to the object it shows \
                 (destinationMedia, EPC §3.4.2 p. 16), and a file with no uuid cannot be traced back",
                shape.name
            ),
        )),
    }
}

/// The object as the exporters see it, rebuilt from what the bag says - or
/// the refusal that names what is missing.
fn scene_object(shape: &Shape) -> Result<SceneObject, Refusal> {
    let (reference, known) = object_of(shape)?;
    // A frame named and not given: the numbers were read placed, or the frame
    // could not be read. Either way the exporter cannot place them.
    if let (Some(frame), None) = (known.crs.as_ref(), known.placement.as_ref()) {
        return Err(Refusal::on(
            "shape",
            format!(
                "`{}` names the frame `{}` and the bag does not say how it places a number - the shape was read \
                 placed, or its frame could not be read; an exporter places the numbers itself, from the local ones",
                reference.title, frame.title
            ),
        ));
    }
    Ok(SceneObject {
        uuid: reference.uuid.clone(),
        // The URI the store addresses the object by, with no dataspace: what
        // `ObjectKey::etp_uri(None)` writes, and what `energyml export` puts in
        // the file.
        uri: reference.uri(),
        title: reference.title.clone(),
        qualified_type: reference.qualified_type.clone(),
        geometry: shape.geometry.clone(),
        crs: known.placement.clone(),
        interpretation: known.interpretation.as_ref().map(|one| Related {
            uuid: one.uuid.clone(),
            title: one.title.clone(),
            qualified_type: one.qualified_type.clone(),
        }),
        feature: None,
        metadata: Default::default(),
        attributes: Vec::new(),
    })
}

/// A title as a file name: lower case, accents folded, anything else a dash -
/// `H3 - top of the Yprésien` → `h3-top-of-the-ypresien`. Empty when nothing
/// of the title survives.
pub fn file_name_of(title: &str) -> String {
    let mut out = String::new();
    for c in title.to_lowercase().chars() {
        let folded = match c {
            'a'..='z' | '0'..='9' | '_' => None,
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => Some("a"),
            'ç' => Some("c"),
            'è' | 'é' | 'ê' | 'ë' => Some("e"),
            'ì' | 'í' | 'î' | 'ï' => Some("i"),
            'ñ' => Some("n"),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => Some("o"),
            'ù' | 'ú' | 'û' | 'ü' => Some("u"),
            'ý' | 'ÿ' => Some("y"),
            'œ' => Some("oe"),
            'æ' => Some("ae"),
            _ => Some("-"),
        };
        match folded {
            None => out.push(c),
            Some("-") => {
                if !out.is_empty() && !out.ends_with('-') {
                    out.push('-');
                }
            }
            Some(letters) => out.push_str(letters),
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Why an exporter declines an object, in the words a person can act on -
/// what `handles_with` answers with a boolean.
fn declined(exporter: &dyn Exporter, object: &SceneObject, options: &Options<'_>) -> String {
    let mut facts = Vec::new();
    if object.geometry.is_empty() {
        facts.push("it has no point".to_string());
    }
    if !options.can_place_vertical(object) {
        facts.push(
            "its vertical axis is a travel time, and an export here states no equivalence to metres"
                .to_string(),
        );
    }
    if object.crs.is_none() {
        facts.push("it names no coordinate system".to_string());
    } else if object.epsg().is_none() {
        facts.push("its frame names no EPSG code".to_string());
    }
    format!(
        "the `{}` exporter does not write `{}` ({}){}",
        exporter.name(),
        object.title,
        object.qualified_type,
        if facts.is_empty() {
            String::new()
        } else {
            format!(": {}", facts.join("; "))
        }
    )
}

impl Operation for ExportMedia {
    type Params = ExportMediaParams;
    type Inputs = ExportMediaInputs;

    fn about(&self) -> About {
        About {
            name: "export-media",
            describe: "a representation written by an exporter - OBJ, GeoJSON, VTK - and put in the package as a part related to it",
            category: Category::Create,
            makes: &[],
            recipe: "export-media",
            represents: None,
        }
    }

    fn check(&self, inputs: &ExportMediaInputs) -> Result<(), Refusal> {
        // An artefact is no object: the operation has nothing to relate its
        // file to. What the frame says is the plan's business.
        object_of(&inputs.shape).map(|_| ())
    }

    fn reports(&self) -> Vec<Slot> {
        vec![
            Slot::required("part", Need::Text, "the part name the export was given"),
            Slot::required(
                "mediaType",
                Need::Text,
                "the content type written for it - the exporter's own",
            ),
            Slot::required(
                "byteCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "its size",
            ),
        ]
    }

    fn plan(
        &self,
        inputs: ExportMediaInputs,
        params: ExportMediaParams,
        _cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let exporters = Exporters::builtin();
        let exporter = exporters
            .get(&params.format)
            .map_err(|why| Refusal::on("format", why))?;
        let object = scene_object(&inputs.shape)?;

        // The one Reprojector the workspace ships. Only a GeoJSON asks for it,
        // and it costs a map to build.
        let backend = energyml_proj::Proj4::new();
        let options = Options::new().with_reprojector(&backend);
        if !exporter.handles_with(&object, &options) {
            return Err(Refusal::on("format", declined(exporter, &object, &options)));
        }
        let mut bytes = Vec::new();
        exporter.write(&object, &mut bytes, &options).map_err(|why| {
            Refusal::on(
                "format",
                format!(
                    "the `{}` exporter refuses `{}`: {why}",
                    exporter.name(),
                    object.title
                ),
            )
        })?;

        let path = match params
            .path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            Some(path) => path.to_string(),
            None => {
                let name = file_name_of(&object.title);
                let name = if name.is_empty() {
                    object.uuid.clone()
                } else {
                    name
                };
                format!("media/export/{name}.{}", exporter.extension())
            }
        };
        let path = check_part_name(&path).map_err(|why| Refusal::on("path", why.to_string()))?;
        // The exporter's word, checked as any content type is: a registry
        // entry brought by a caller could spell it wrong.
        let content_type = check_media_type(exporter.mime()).map_err(|why| {
            Refusal::on(
                "format",
                format!("the `{}` exporter declares {why}", exporter.name()),
            )
        })?;

        let (reference, _) = object_of(&inputs.shape)?;
        let uuid = reference.uuid.to_ascii_lowercase();
        let says = vec![
            Note {
                key: "media",
                detail: format!(
                    "`{path}`, {content_type} (the `{}` exporter's), {} byte(s)",
                    exporter.name(),
                    bytes.len()
                ),
            },
            Note {
                key: "shows",
                detail: format!("destinationMedia from `{}`, sourceMedia back", reference.title),
            },
            Note {
                key: "identity",
                detail: format!(
                    "the file names the object{} and no feature: the bag carries the interpretation's reference, \
                     not what it interprets; and no property is written",
                    if object.interpretation.is_some() {
                        " and its interpretation"
                    } else {
                        ""
                    }
                ),
            },
        ];
        let parameters = vec![
            Parameter::Text {
                title: "part".to_string(),
                value: path.clone(),
            },
            Parameter::Text {
                title: "mediaType".to_string(),
                value: content_type.clone(),
            },
            Parameter::Double {
                title: "byteCount".to_string(),
                value: bytes.len() as f64,
                uom: None,
            },
            Parameter::Object {
                title: "input".to_string(),
                reference: reference.dor(),
            },
        ];
        Ok(Plan {
            blocked: None,
            objects: Vec::new(),
            datasets: Vec::new(),
            media: vec![Part {
                path: path.clone(),
                content_type,
                bytes,
                related_to: vec![uuid.clone()],
            }],
            activity: ActivityDraft {
                recipe: "export-media",
                title: format!("export-media → {path}"),
                seed: format!("export-media:{path}:{uuid}"),
                parameters,
            },
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}
