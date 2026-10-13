//! `attach` - a file attached to an object, or to a collection, in the form
//! the destination can hold.
//!
//! The attachment operation has to be offered in the operations list and also
//! in the view that adds and lists the package's internal media - one operation, three
//! doors (the operations list, the package's files, an object's sheet), each
//! pre-filling what it knows.
//!
//! # Four forms, one reader
//!
//! | form | written here | where the bytes go |
//! |---|---|---|
//! | **part** (A) | what `media` writes - **delegated**, not rewritten | a part of the zip, `destinationMedia` / `sourceMedia` (EPC §3.4.2 pp. 15-16) |
//! | **inline** (B) | an `eml23.Attachment`, `Content` in base 64 | the XML of the object |
//! | **part-address** (C) | an `eml23.Attachment` with `Content` empty and `ContentUri` a part name, **plus the part** (by `media`, related to the `Attachment` itself: `destinationMedia`) | the zip |
//! | **external** (D) | an `eml23.Attachment` with `Content` empty and `ContentUri` an http(s) address | a server - never fetched here |
//!
//! `energyml_domain::attachments` reads all four back, and the test confronts
//! the two: what is written here is what that reader finds.
//!
//! # What is decided, and what is refused
//!
//! - **the form follows what is given** when it is not stated: a file is a
//!   part (A - it passes everywhere and grows no object), an address alone is
//!   external (D). A form stated that the inputs cannot make is refused on its
//!   slot: a file for D, an address for A, B or C;
//! - **an `Attachment` is an EML 2.3 object**, so B, C and D are refused on a
//!   target of a release that has none (RESQML 2.0.1, EML 2.0 - a type whose
//!   package number ends in `20`), **by name, with the part form offered**;
//! - **a part has no depth and no category** (A is the packaging conventions'
//!   relation, which carries neither): `md` and `category` with the part form
//!   are refused, and the forms that carry them are named;
//! - **a depth goes on what has a measured depth**: a well, a trajectory, a
//!   frame, a log, or a collection (which may hold wells - a depth of 0 on a
//!   collection puts the image at the head of the hole). A surface with an
//!   `md` is refused. `Md` is **the head of the hole** at 0 - the datum, not
//!   necessarily the ground - and the unit is written with it;
//! - **the content type is stated, never guessed** (the rule of `media`): the
//!   one the source declared or `contentType`; required for the three forms
//!   that carry a file, optional for an address;
//! - **an address is `http` or `https`**: another scheme is a refusal that
//!   names it, since no reader here follows it;
//! - **`FileName` and `Category` hold 64 characters at most** (`Attachment.xsd`),
//!   and a longer one is refused rather than cut.
//!
//! # A document named with a folder is a part already
//!
//! The package's files view hands a part over **by its part name**
//! (`media/symbols/x.png`) to be related to an object. Written under the default
//! `media/` + name it would be a second copy, so a document whose name holds a
//! folder keeps that name as its part name - unless `path` says otherwise. A
//! file picked from a disk has no folder in its name; the inline form's
//! `FileName` is always the last segment (*a name only, not a path*).
//!
//! # The convention of form C
//!
//! The schema says `ContentUri` is *« A URI pointing to the location of the
//! attached content »* and nothing about the zip. Reading a relative address as
//! a part name is **this tool's convention**, and the plan says so (note
//! `convention`). Whether a full package URI should
//! be written instead is an open question; the part name is written, and both are meant to be read.
//!
//! # Identity
//!
//! The same target, form, name, depth, category **and bytes** are the same
//! `Attachment` uuid: a second run writes the same object, another file under
//! the same name another.

use energyml_core::xml::XmlText;
use energyml_domain::attachments::encode_base64;
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_storage::epc::media::check_media_type;

use super::media::{Document, Media, MediaInputs, MediaParams};
use crate::declare::{Choice, InputParam, Inputs, ObjectIn, Params, Title};
use crate::operation::{
    About, ActivityDraft, Category, Context, Need, NewObject, Note, Operation, Plan, Refusal, Slot,
};

/// The operation.
pub struct Attach;

/// The form of the attachment - the four forms of the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Form {
    /// A part of the package, related to the object (the packaging conventions).
    Part,
    /// An `Attachment` carrying the file in its `Content`, in base 64.
    Inline,
    /// An `Attachment` whose `ContentUri` names a part of the package.
    PartAddress,
    /// An `Attachment` whose `ContentUri` is an external address.
    External,
}

/// What `attach` is told.
#[derive(Debug, Params)]
pub struct AttachParams {
    /// how the attachment is written: the part of the package (A), the file inside the object (B), an address to a part (C) or to a server (D) - left out, a file is a part and an address is external
    pub form: Option<Form>,
    /// the external address of the file, http or https - instead of a file, for the external form
    pub address: Option<String>,
    /// the measured depth the file is indexed at, along the hole - 0 is the head of the hole; left out, the file is about the whole object
    pub md: Option<f64>,
    /// the unit of `md`
    #[param(default = String::from("m"), optional)]
    pub md_uom: String,
    /// what the file is, in a word - `Symbol`, `HeadImage`, `CuttingsPicture` (64 characters at most)
    pub category: Option<String>,
    /// the file's name, without a folder - left out, the file's own (64 characters at most)
    pub file_name: Option<String>,
    /// the file's content type, `image/png` - left out, what its source declared; never guessed from the name
    pub content_type: Option<String>,
    /// for the part and part-address forms: the part's name in the package - left out, `media/` and the file's name
    pub path: Option<String>,
    /// the attachment's title - left out, the file's name and what it is attached to
    pub title: Option<Title>,
}

/// What `attach` takes.
#[derive(Debug, Inputs)]
pub struct AttachInputs {
    /// the object or collection the file is attached to - what is attached to a collection applies to its members
    pub target: ObjectIn,
    /// the file, carried as bytes - left out when only an address is given
    pub file: Option<Document>,
}

impl InputParam for Option<Document> {
    type Item = Document;
    fn least() -> u32 {
        0
    }
    fn most() -> Option<u32> {
        Some(1)
    }
    fn from_items(items: Vec<Document>) -> Self {
        items.into_iter().next()
    }
}

/// The type name without its package: `resqml22.WellboreFeature` → `WellboreFeature`.
fn bare(qualified: &str) -> &str {
    let name = qualified.split_once('.').map_or(qualified, |(_, name)| name);
    name.strip_prefix("obj_").unwrap_or(name)
}

/// Whether the package of a type has the `Attachment` object: EML 2.3 and what
/// builds on it. A package number ending in `20` (`resqml20`, `eml20`,
/// `witsml20`) is a release before it.
fn knows_attachment(qualified: &str) -> bool {
    let package = qualified.split('.').next().unwrap_or_default();
    !package.ends_with("20")
}

/// Whether the target can carry a depth along a hole.
fn has_depth(qualified: &str) -> bool {
    let name = bare(qualified);
    ["Well", "Trajectory", "Log", "Frame"]
        .iter()
        .any(|word| name.contains(word))
        || name == "DataobjectCollection"
}

/// 64-bit FNV-1a: a name for the bytes in a seed, no more.
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl Operation for Attach {
    type Params = AttachParams;
    type Inputs = AttachInputs;

    fn about(&self) -> About {
        About {
            name: "attach",
            describe: "a file attached to an object or a collection - a part of the package, or an Attachment carrying the file, naming a part or an address",
            category: Category::Create,
            makes: &["eml23.Attachment"],
            recipe: "attach",
            represents: None,
        }
    }

    fn reports(&self) -> Vec<Slot> {
        vec![
            Slot::optional(
                "part",
                Need::Text,
                "the part name the file was given, when it is in the package",
            ),
            Slot::optional("mediaType", Need::Text, "the content type written for it"),
            Slot::optional(
                "byteCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "the file's size",
            ),
        ]
    }

    fn plan(&self, inputs: AttachInputs, params: AttachParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let target = inputs.target;
        let Some(reference) = target.known.reference.clone() else {
            return Err(Refusal::on(
                "target",
                format!("`{}` was given without a reference to attach to", target.name),
            ));
        };
        let address = params
            .address
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());

        // -- what is given decides the form, or is refused against the one asked
        let form = match (params.form, inputs.file.is_some(), address.is_some()) {
            (_, false, false) => {
                return Err(Refusal::on(
                    "file",
                    "nothing to attach: give a file, or an `address` for the external form",
                ));
            }
            (_, true, true) => {
                return Err(Refusal::on(
                    "address",
                    "a file and an address are both given: the file is written in the package or the object, and the address names a file that stays where it is - give one",
                ));
            }
            (None, true, false) => Form::Part,
            (None, false, true) => Form::External,
            (Some(Form::External), true, _) => {
                return Err(Refusal::on(
                    "form",
                    "the external form names a file that stays where it is: give its `address`, not the file",
                ));
            }
            (Some(form @ (Form::Part | Form::Inline | Form::PartAddress)), false, _) => {
                return Err(Refusal::on(
                    "form",
                    format!(
                        "the {} form writes the file: give it as `file` - an address alone is the external form",
                        form.word()
                    ),
                ));
            }
            (Some(form), _, _) => form,
        };

        // -- the release: an Attachment is an EML 2.3 object
        if form != Form::Part && !knows_attachment(&reference.qualified_type) {
            return Err(Refusal::on(
                "form",
                format!(
                    "`{}` is a {}, of a release that has no `Attachment` object (EML 2.3): the {} form cannot name it - the part form (`part`) relates a file to it in the package",
                    reference.title,
                    reference.qualified_type,
                    form.word()
                ),
            ));
        }

        // -- what a part cannot carry
        if form == Form::Part {
            for (slot, given) in [
                ("md", params.md.is_some()),
                ("category", params.category.is_some()),
            ] {
                if given {
                    return Err(Refusal::on(
                        slot,
                        "a part related to an object carries neither a depth nor a category (EPC §3.4.2 pp. 15-16): the inline, part-address and external forms do",
                    ));
                }
            }
        }
        if params.md.is_some() && !has_depth(&reference.qualified_type) {
            return Err(Refusal::on(
                "md",
                format!(
                    "`{}` is a {}, which has no measured depth: a depth indexes a file along a well, a trajectory, a frame, a log - or a collection that may hold wells",
                    reference.title, reference.qualified_type
                ),
            ));
        }
        let md = match params.md {
            None => None,
            Some(value) if !value.is_finite() => {
                return Err(Refusal::on("md", format!("{value} is not a depth")));
            }
            Some(value) => {
                let uom = params.md_uom.trim();
                // The enumeration is open - a word it does not list is kept as
                // `Other` - so a unit that is not the standard list's is
                // refused here, where it would otherwise be written as typed.
                let uom = match <eml::LengthUomExt as XmlText>::from_xml_text(uom) {
                    Ok(eml::LengthUomExt::Other(_)) | Err(_) => {
                        return Err(Refusal::on(
                            "mdUom",
                            format!("`{uom}` is not a unit of length of the standard list (`m`, `ft`, …)"),
                        ));
                    }
                    Ok(listed) => listed,
                };
                Some(eml::MeasuredDepth {
                    measured_depth: eml::LengthMeasureExt { value, uom },
                    datum: None,
                })
            }
        };

        // -- the file's names
        let named = |slot: &str, value: &Option<String>| -> Result<Option<String>, Refusal> {
            match value.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
                Some(text) if text.chars().count() > 64 => Err(Refusal::on(
                    slot,
                    format!(
                        "{} characters, and the schema holds 64 (`Attachment.xsd`): shorten it, a name is never cut here",
                        text.chars().count()
                    ),
                )),
                Some(text) => Ok(Some(text.to_string())),
                None => Ok(None),
            }
        };
        let category = named("category", &params.category)?;
        let stated_name = named("fileName", &params.file_name)?;
        if let Some(name) = &stated_name
            && (name.contains('/') || name.contains('\\'))
        {
            return Err(Refusal::on(
                "fileName",
                "a name only, not a path (`Attachment.xsd`: *NOT a file path*)",
            ));
        }

        // -- the content type: stated, never guessed
        let stated_type = params
            .content_type
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty());
        let content_type = |declared: Option<&str>,
                            file: &str,
                            required: bool|
         -> Result<Option<String>, Refusal> {
            match stated_type.or(declared) {
                Some(kind) => check_media_type(kind)
                    .map(Some)
                    .map_err(|why| Refusal::on("contentType", why.to_string())),
                None if required => Err(Refusal::on(
                    "contentType",
                    format!(
                        "the content type of `{file}` is stated nowhere - give `contentType`; it is never guessed from a file's name"
                    ),
                )),
                None => Ok(None),
            }
        };

        // A document named with a folder is **already a part of the package**
        // - `media/symbols/x.png`, as the files view hands one over to be
        // related to an object: its name is its part name, and the default
        // `media/` + name would write it a second time under another one. A
        // file from a disk has no folder in its name.
        let part_path = params.path.clone().or_else(|| {
            inputs
                .file
                .as_ref()
                .and_then(|file| file.file.contains('/').then(|| file.file.clone()))
        });
        let base_name = |file: &str| file.rsplit('/').next().unwrap_or(file).to_string();

        let mut says: Vec<Note> = Vec::new();
        let title_of = |name: &str| match &params.title {
            Some(title) if !title.0.trim().is_empty() => title.0.trim().to_string(),
            _ => format!("{name} on {}", reference.title),
        };

        // -- A: what `media` writes, and nothing else
        if form == Form::Part {
            let file = inputs.file.expect("a file is given for the part form");
            let mut plan = Media.plan(
                MediaInputs {
                    file,
                    shows: vec![target.clone()],
                },
                MediaParams {
                    path: part_path.clone(),
                    content_type: params.content_type.clone(),
                },
                cx,
            )?;
            plan.activity.recipe = "attach";
            plan.activity.seed = format!("attach:{}", plan.activity.seed);
            plan.activity.title = format!(
                "attach → {}",
                plan.media.first().map_or("", |part| part.path.as_str())
            );
            plan.activity.parameters.insert(
                0,
                Parameter::Text {
                    title: "form".to_string(),
                    value: form.word().to_string(),
                },
            );
            plan.says.insert(
                0,
                Note {
                    key: "form",
                    detail: "the part form: a file of the package, related to the object".to_string(),
                },
            );
            return Ok(plan);
        }

        // -- B, C, D: an Attachment
        let mut parameters: Vec<Parameter> = vec![
            Parameter::Text {
                title: "form".to_string(),
                value: form.word().to_string(),
            },
            Parameter::Object {
                title: "input".to_string(),
                reference: reference.dor(),
            },
        ];
        let mut media: Vec<crate::operation::Media> = Vec::new();
        let (content, content_uri, file_type, file_name, fingerprint_of): (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
        ) = match form {
            Form::Inline => {
                let file = inputs.file.as_ref().expect("a file is given");
                let kind = content_type(file.declared.as_deref(), &file.file, true)?;
                let name = stated_name.clone().or_else(|| Some(base_name(&file.file)));
                says.push(Note {
                        key: "form",
                        detail: format!(
                            "the inline form: {} byte(s) in the object's `Content`, in base 64 - the object grows by a third",
                            file.bytes.len()
                        ),
                    });
                parameters.push(Parameter::Double {
                    title: "byteCount".to_string(),
                    value: file.bytes.len() as f64,
                    uom: None,
                });
                (
                    encode_base64(&file.bytes),
                    None,
                    kind,
                    name,
                    format!("{:016x}", fingerprint(&file.bytes)),
                )
            }
            Form::PartAddress => {
                let file = inputs.file.as_ref().expect("a file is given");
                let kind = content_type(file.declared.as_deref(), &file.file, true)?;
                // The part is written by `media`. It is related to the
                // **Attachment** (set below, once its uuid is derived) and
                // not to the target: the target's pieces are found through
                // the Attachment's `ObjectReference`, and a relation from
                // the target too would list the file twice for it.
                let part = Media.plan(
                    MediaInputs {
                        file: file.clone(),
                        shows: Vec::new(),
                    },
                    MediaParams {
                        path: part_path.clone(),
                        content_type: params.content_type.clone(),
                    },
                    cx,
                )?;
                let written = part.media.first().expect("`media` writes its part").clone();
                says.push(Note {
                    key: "form",
                    detail: format!(
                        "the part-address form: the file is the part `{}`, the attachment names it",
                        written.path
                    ),
                });
                says.push(Note {
                        key: "convention",
                        detail: "`ContentUri` names a part of the package by its path: this tool's reading of a relative address - the schema says only *a URI pointing to the location of the attached content*".to_string(),
                    });
                parameters.push(Parameter::Text {
                    title: "part".to_string(),
                    value: written.path.clone(),
                });
                parameters.push(Parameter::Double {
                    title: "byteCount".to_string(),
                    value: written.bytes.len() as f64,
                    uom: None,
                });
                let name = stated_name.clone().or_else(|| Some(base_name(&file.file)));
                let print = format!("{:016x}", fingerprint(&written.bytes));
                let uri = written.path.clone();
                media.push(crate::operation::Media {
                    related_to: Vec::new(),
                    ..written
                });
                // (`related_to` is filled below, with the Attachment's uuid.)
                (String::new(), Some(uri), kind, name, print)
            }
            Form::External => {
                let url = address.expect("an address is given");
                let lower = url.to_ascii_lowercase();
                if !(lower.starts_with("http://") || lower.starts_with("https://")) {
                    let scheme = url.split(':').next().unwrap_or(url);
                    return Err(Refusal::on(
                        "address",
                        format!(
                            "`{scheme}` is not a scheme that is read: an external address is http or https"
                        ),
                    ));
                }
                if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    return Err(Refusal::on(
                        "address",
                        "an address holds no space and no control character",
                    ));
                }
                let kind = content_type(None, url, false)?;
                let tail = url
                    .split(['?', '#'])
                    .next()
                    .unwrap_or(url)
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or(url);
                let name = stated_name
                    .clone()
                    .or_else(|| (!tail.is_empty() && tail.chars().count() <= 64).then(|| tail.to_string()));
                says.push(Note {
                        key: "form",
                        detail: "the external form: the attachment names an address, and nothing is fetched - a reader follows it on request".to_string(),
                    });
                parameters.push(Parameter::Text {
                    title: "address".to_string(),
                    value: url.to_string(),
                });
                (String::new(), Some(url.to_string()), kind, name, "-".to_string())
            }
            Form::Part => unreachable!("the part form returned above"),
        };
        if let Some(kind) = &file_type {
            parameters.push(Parameter::Text {
                title: "mediaType".to_string(),
                value: kind.clone(),
            });
        }

        let name_for_title = file_name
            .clone()
            .or_else(|| content_uri.clone())
            .unwrap_or_else(|| "file".to_string());
        let title = title_of(&name_for_title);
        let seed = format!(
            "attach:{}:{}:{}:{}:{}:{}",
            reference.uuid.to_ascii_lowercase(),
            form.word(),
            file_name.clone().unwrap_or_default(),
            md.as_ref()
                .map(|md| format!(
                    "{}{}",
                    md.measured_depth.value,
                    md.measured_depth.uom.as_xml_str()
                ))
                .unwrap_or_default(),
            category.clone().unwrap_or_default(),
            fingerprint_of
        );
        let uuid = cx.identity.uuid(&seed, 0)?;
        // Form C: the Attachment is the object the part is a media
        // representation of - `destinationMedia` from its `.rels`,
        // `sourceMedia` back (Packaging Conventions v1.0 §3.4.2 pp. 15-16:
        // *« The object in Target is a media representation for the current
        // object »*). Without it the part is in the zip and nothing in the
        // package's relations says what it is for (the part would otherwise
        // appear related to no image file in the EPC).
        for part in &mut media {
            part.related_to = vec![uuid.to_ascii_lowercase()];
        }
        if let Some(md) = &md {
            says.push(Note {
                key: "md",
                detail: format!(
                    "indexed at {} {}{}",
                    md.measured_depth.value,
                    md.measured_depth.uom.as_xml_str(),
                    if md.measured_depth.value == 0.0 {
                        " - the head of the hole, at the datum"
                    } else {
                        ""
                    }
                ),
            });
        }
        if reference.qualified_type.ends_with("DataobjectCollection") {
            says.push(Note {
                key: "collection",
                detail: "attached to a collection: it applies to the members the inheritance order reaches, and is listed with where it came from".to_string(),
            });
        }

        let attachment = eml::Attachment {
            uuid: uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: eml::Citation {
                title: title.clone(),
                originator: cx.when.originator.clone(),
                creation: cx.when.creation.clone(),
                format: energyml_core::CITATION_FORMAT.to_string(),
                ..Default::default()
            },
            object_reference: Some(crate::crs::reference_of(&reference)),
            md,
            category,
            file_name,
            file_type,
            content_uri,
            content,
            ..Default::default()
        };

        Ok(Plan {
            blocked: None,
            objects: vec![NewObject {
                uuid,
                qualified_type: "eml23.Attachment".to_string(),
                role: "attachment".to_string(),
                title: title.clone(),
                json: energyml_core::json::write_value(&attachment),
            }],
            datasets: Vec::new(),
            media,
            activity: ActivityDraft {
                recipe: "attach",
                title: format!("attach → {title}"),
                seed,
                parameters,
            },
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}
