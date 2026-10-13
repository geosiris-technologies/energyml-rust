//! `media` - a file put **in the package**, beside the objects, and related to
//! the objects it shows.
//!
//! The packaging conventions allow it: the files go into the zip of the EPC and are
//! referenced from the content types (ADR-0014).
//!
//! The packaging conventions do allow the files
//! (Energistics Packaging Conventions v1.0, §3.2
//! pp. 11-12: "other files that contain additional, informal information …
//! As a guideline these files are stored in a folder named `media`"; §3.3
//! p. 14: "An optional media folder may contain any type of media file--no
//! restrictions"). **The content type is not what refers to them**, though: it
//! types the part (§2.2.1 p. 9); the object a file is about is named by an OPC
//! relationship, `destinationMedia` from the object and `sourceMedia` back
//! (§3.4.2 pp. 15-16). This operation plans both halves; `energyml-storage`
//! writes them (`EpcStore::set_attachment`, `EpcStore::relate_media`).
//!
//! # What is decided here, and what is refused
//!
//! - **the file is carried as bytes**, never read: a PDF is not text, and what
//!   an EPC holds is the file, not an interpretation of it;
//! - **the content type is stated, never guessed** - by `contentType`, or by
//!   the server the file came from. A file from a disk declares none, and its
//!   name is not evidence (the registry rule: the types come from the exporter, not from a second table). Neither is refused by name;
//!
//! - **the part name follows the rules of the package**, checked by the same
//!   function the store applies (`energyml_storage::epc::media`): a plan that
//!   would be refused at `apply` is refused at `plan`, for the same reason and
//!   in the same words;
//! - **a file related to nothing is written and said** (`unrelated`): §3.4.1
//!   p. 15 makes the relationship optional, and a README of the whole package
//!   shows no object in particular;
//! - **an object given twice is related once**, and counted.
//!
//! No object is made, so no uuid is derived; the activity records the part,
//! its type, its size and the objects it shows (`input`).

use std::collections::BTreeSet;

use energyml_domain::provenance::Parameter;
use energyml_storage::epc::media::{check_media_type, check_part_name};

use crate::declare::{Input, InputParam, Inputs, ObjectIn, Params, Piece};
use crate::operation::Context;
use crate::operation::{
    About, ActivityDraft, Category, Media as Part, Need, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::Source;

/// The operation.
pub struct Media;

/// A file, as bytes - the input a `media` step takes.
#[derive(Debug, Clone)]
pub struct Document {
    /// Its name in the bag - the slot that received it.
    pub name: String,
    /// The file it came from - `README.md`.
    pub file: String,
    /// The content type its source declared, if any.
    pub declared: Option<String>,
    /// The bytes.
    pub bytes: Vec<u8>,
}

impl Input for Document {
    const KIND: &'static str = "document";
    fn from_piece(piece: &Piece<'_>) -> Result<Self, String> {
        match piece.source {
            Source::Document {
                name,
                content_type,
                bytes,
            } => Ok(Document {
                name: piece.name.to_string(),
                file: name.clone(),
                declared: content_type.clone(),
                bytes: bytes.clone(),
            }),
            other => Err(format!(
                "`{}` is {}, and a file to carry is wanted",
                piece.name,
                other.kind()
            )),
        }
    }
}

impl InputParam for Document {
    type Item = Document;
    fn least() -> u32 {
        1
    }
    fn most() -> Option<u32> {
        Some(1)
    }
    fn from_items(mut items: Vec<Document>) -> Self {
        items.remove(0)
    }
}

/// What `media` is told.
#[derive(Debug, Params)]
pub struct MediaParams {
    /// the part name in the package - left out, `media/` and the file's name (EPC §3.2: "a folder named media")
    pub path: Option<String>,
    /// the file's content type, `text/markdown` or `application/pdf` - left out, what its source declared; never guessed from the name
    pub content_type: Option<String>,
}

/// What `media` takes.
#[derive(Debug, Inputs)]
pub struct MediaInputs {
    /// the file to put in the package, carried as bytes
    pub file: Document,
    /// the objects the file is a media representation of - EPC's destinationMedia
    pub shows: Vec<ObjectIn>,
}

impl Operation for Media {
    type Params = MediaParams;
    type Inputs = MediaInputs;

    fn about(&self) -> About {
        About {
            name: "media",
            describe: "a file put in the package - a README, a PDF, an export - related to the objects it shows",
            category: Category::Create,
            makes: &[],
            recipe: "media",
            represents: None,
        }
    }

    fn reports(&self) -> Vec<Slot> {
        vec![
            Slot::required("part", Need::Text, "the part name the file was given"),
            Slot::required("mediaType", Need::Text, "the content type written for it"),
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

    fn plan(&self, inputs: MediaInputs, params: MediaParams, _cx: &Context<'_>) -> Result<Plan, Refusal> {
        let file = inputs.file;
        let path = match params
            .path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            Some(path) => path.to_string(),
            None => format!("media/{}", file.file),
        };
        let path = check_part_name(&path).map_err(|why| Refusal::on("path", why.to_string()))?;
        let stated = params
            .content_type
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty());
        let Some(content_type) = stated.or(file.declared.as_deref()) else {
            return Err(Refusal::on(
                "contentType",
                format!(
                    "the content type of `{}` is stated nowhere - give `contentType`; a package types every part \
                     (EPC §2.2.1 p. 9), and a type is never guessed from a file's name",
                    file.file
                ),
            ));
        };
        let content_type =
            check_media_type(content_type).map_err(|why| Refusal::on("contentType", why.to_string()))?;

        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut related: Vec<String> = Vec::new();
        let mut shown: Vec<Parameter> = Vec::new();
        let mut again = 0usize;
        for object in &inputs.shows {
            let Some(reference) = object.known.reference.as_ref() else {
                return Err(Refusal::on(
                    "shows",
                    format!("`{}` was given without a reference to relate", object.name),
                ));
            };
            if !seen.insert(reference.uuid.to_ascii_lowercase()) {
                again += 1;
                continue;
            }
            related.push(reference.uuid.to_ascii_lowercase());
            shown.push(Parameter::Object {
                title: "input".to_string(),
                reference: reference.dor(),
            });
        }

        let mut says = vec![Note {
            key: "media",
            detail: format!("`{path}`, {content_type}, {} byte(s)", file.bytes.len()),
        }];
        if related.is_empty() {
            says.push(Note {
                key: "unrelated",
                detail: "the file shows no object: it is written with no destinationMedia (optional, EPC §3.4.1 p. 15)"
                    .to_string(),
            });
        } else {
            says.push(Note {
                key: "shows",
                detail: format!(
                    "destinationMedia from {} object(s), sourceMedia back",
                    related.len()
                ),
            });
        }
        if again > 0 {
            says.push(Note {
                key: "twice",
                detail: format!("{again} object(s) given more than once, related once"),
            });
        }

        let mut parameters = vec![
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
                value: file.bytes.len() as f64,
                uom: None,
            },
        ];
        parameters.extend(shown);

        Ok(Plan {
            blocked: None,
            objects: Vec::new(),
            datasets: Vec::new(),
            media: vec![Part {
                path: path.clone(),
                content_type,
                bytes: file.bytes,
                related_to: related.clone(),
            }],
            activity: ActivityDraft {
                recipe: "media",
                title: format!("media → {path}"),
                seed: format!("media:{path}:{}", related.join(",")),
                parameters,
            },
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}
