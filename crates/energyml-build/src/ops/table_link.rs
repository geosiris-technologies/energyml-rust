//! `table-link` - write, on an explicit gesture, the link of a column to a
//! table, or of a property to the table of its axes, **in convention B**.
//!
//! Convention B (the readers' first, `energyml_domain::table::links`): the
//! column has the kind `identifier` (a link by key) or `explicit index` /
//! `classification index` (by rank), and carries **one** `Facet` of kind
//! `qualifier` whose value is the **full ETP uri** of the target table - the
//! version included, never a bare uuid. The target has **one** `KeyColumn` for
//! a link by key, or the link is refused. The value of a facet of an extension
//! follows `EnumExtensionPattern` (`.*:.*`, no maximum length): a uri passes.
//!
//! Convention A (the target written in the `Description`) is **derived** from
//! B for a human reader, in the exact grammar the reader of
//! `energyml_domain::table::links` reads, and only when the column has no
//! description. A link by rank has no form A.
//!
//! **The kind is never changed in silence.** A column whose kind is no
//! identifier is refused, naming the kind and the operation that sets it
//! (`edit-object`).
//!
//! The reading of these links is the domain's (`LinkReaders`,
//! `property::qualifier_link`); this operation writes only what their reader
//! reads first. The writing is the one of [`super::edit_object`].
//!
//! # References
//!
//! - Energistics common Technical Reference Guide v2.3 (2022), section 11.11 p. 294
//!   (`PropertyKindFacet`), section 11.3 p. 285 (`FacetKind`), section 11.2
//!   p. 284 (`FacetExt`).

use energyml_core::registry::{DynObject, Registry};
use energyml_domain::property_kind::Resolver;
use energyml_domain::provenance::Parameter;
use energyml_domain::source::ObjectSource;
use energyml_domain::table::TableSheet;
use energyml_domain::table::class::{EXPLICIT_INDEX, IDENTIFIER, descends_from, resolve_kinds};
use serde_json::{Value, json};

use crate::declare::{Choice, Inputs, ObjectIn, Params};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};

use super::edit_object::{EditObject, EditObjectInputs, EditObjectParams};

/// The operation.
pub struct TableLink;

/// How a value of the column names a row of the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum LinkBy {
    /// by the value of the target's one key column
    Key,
    /// by the row's rank, as `table::row_named_by` reads it
    Rank,
}

/// What `table-link` is told.
#[derive(Debug, Params)]
pub struct TableLinkParams {
    /// the title of the column that holds the link - when `object` is a table; left out for a property
    pub column: Option<String>,
    /// how a value names a row of the target
    #[param(default = LinkBy::Key, optional)]
    pub by: LinkBy,
}

/// What `table-link` takes.
#[derive(Debug, Inputs)]
pub struct TableLinkInputs {
    /// the table whose column links, or the property whose component axes the target holds
    #[input(
        types = "eml23.ColumnBasedTable, resqml22.ContinuousProperty, resqml22.DiscreteProperty, resqml22.CategoricalProperty"
    )]
    pub object: ObjectIn,
    /// the table linked to
    #[input(types = "eml23.ColumnBasedTable")]
    pub target: ObjectIn,
}

/// The objects of the bag that came with their content, as a source of
/// objects: what a property kind defined in the package resolves through.
struct BagObjects {
    objects: Vec<(String, Box<dyn DynObject>)>,
}

impl ObjectSource for BagObjects {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.objects
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(uuid))
            .map(|(_, object)| object.clone_box())
    }
}

fn registry() -> Registry {
    let mut types = Registry::new();
    energyml_eml_2_3::register(&mut types);
    energyml_resqml_2_2::register(&mut types);
    types
}

/// An object of the bag whole, read from its JSON; the refusal names `slot`.
fn read_whole(
    json: &Value,
    slot: &str,
    title: &str,
    types: &Registry,
) -> Result<Box<dyn DynObject>, Refusal> {
    energyml_core::json::read_value(json, types)
        .map(|document| document.root)
        .map_err(|why| Refusal::on(slot, format!("`{title}` does not read as an object: {why}")))
}

/// The local name of a qualified type, without a `obj_` prefix.
fn local_name(qualified: &str) -> &str {
    let local = qualified.rsplit('.').next().unwrap_or(qualified);
    local.strip_prefix("obj_").unwrap_or(local)
}

/// The uuid an address names, lower-cased.
fn uuid_of(address: &str) -> String {
    energyml_core::identity::parse_address(address)
        .0
        .to_ascii_lowercase()
}

/// The list a path (`Column[2].Facet`, `Facet`) leads to in an object's JSON;
/// empty when the path leads nowhere.
fn facets_at(object: &Value, path: &str) -> Vec<Value> {
    let mut here = object;
    for step in path.split('.') {
        let (name, rank) = match step.split_once('[') {
            Some((name, rest)) => (name, rest.trim_end_matches(']').parse::<usize>().ok()),
            None => (step, None),
        };
        here = match here.get(name) {
            Some(next) => next,
            None => return Vec::new(),
        };
        if let Some(rank) = rank {
            here = match here.get(rank) {
                Some(next) => next,
                None => return Vec::new(),
            };
        }
    }
    here.as_array().cloned().unwrap_or_default()
}

/// The uri a description of form A names, when it is one - what sits between
/// `: ` and `, KeyColumn`.
fn described_uri(text: &str) -> Option<&str> {
    let after = &text[text.find("eml:")?..];
    Some(after.split(", KeyColumn").next().unwrap_or(after).trim())
}

impl Operation for TableLink {
    type Params = TableLinkParams;
    type Inputs = TableLinkInputs;

    fn about(&self) -> About {
        About {
            name: "table-link",
            describe: "the link of a column to a table, or of a property to the table of its axes, written as a kind and one qualifier facet holding the table's uri",
            category: Category::Create,
            makes: &[],
            recipe: "table-link",
            represents: None,
        }
    }

    fn plan(
        &self,
        inputs: TableLinkInputs,
        params: TableLinkParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let (object, target) = (&inputs.object, &inputs.target);
        let arrived = |slot: &str| {
            Refusal::on(
                slot,
                "arrived without its body - a link is written into the object as it stands",
            )
        };
        let object_ref = object.known.reference.as_ref().ok_or_else(|| arrived("object"))?;
        let object_json = object.known.json.as_ref().ok_or_else(|| arrived("object"))?;
        let target_ref = target.known.reference.as_ref().ok_or_else(|| arrived("target"))?;
        let target_json = target.known.json.as_ref().ok_or_else(|| arrived("target"))?;
        // The full uri, the version included: never the uuid alone.
        let uri = target_ref.uri();

        let types = registry();
        let target_read = read_whole(target_json, "target", &target_ref.title, &types)?;
        let target_sheet = TableSheet::of(target_read.as_reflect()).map_err(|why| {
            Refusal::on(
                "target",
                format!("`{}` is not a table that reads: {why}", target_ref.title),
            )
        })?;
        let keys: Vec<&str> = target_sheet
            .columns
            .iter()
            .filter(|column| column.key)
            .map(|column| column.title.as_deref().unwrap_or(""))
            .collect();
        if params.by == LinkBy::Key && keys.len() != 1 {
            return Err(Refusal::on(
                "target",
                format!(
                    "has {} key column(s): a link by key names the row by one (S3)",
                    keys.len()
                ),
            ));
        }

        let is_table = local_name(&object_ref.qualified_type) == "ColumnBasedTable";
        let mut says = Vec::new();
        // Where the facets live in the object, and the column's own facts.
        let (facet_path, description_path, description): (String, Option<String>, Option<String>) =
            if is_table {
                let title = params.column.as_deref().ok_or_else(|| {
                    Refusal::on(
                        "column",
                        "a table's link is written in one of its columns: name it",
                    )
                })?;
                let object_read = read_whole(object_json, "object", &object_ref.title, &types)?;
                let sheet = TableSheet::of(object_read.as_reflect()).map_err(|why| {
                    Refusal::on(
                        "object",
                        format!("`{}` is not a table that reads: {why}", object_ref.title),
                    )
                })?;
                let found: Vec<usize> = sheet
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(_, column)| column.title.as_deref().map(str::trim) == Some(title.trim()))
                    .map(|(at, _)| at)
                    .collect();
                let at = match found.as_slice() {
                    [one] => *one,
                    [] => {
                        let titles: Vec<&str> = sheet
                            .columns
                            .iter()
                            .map(|column| column.title.as_deref().unwrap_or("(untitled)"))
                            .collect();
                        return Err(Refusal::on(
                            "column",
                            format!(
                                "`{}` has no column titled `{title}` - its columns: {}",
                                object_ref.title,
                                titles.join(", ")
                            ),
                        ));
                    }
                    several => {
                        return Err(Refusal::on(
                            "column",
                            format!(
                                "several columns are titled `{title}` ({} of them): the link cannot name one",
                                several.len()
                            ),
                        ));
                    }
                };
                // The kind, resolved through the package then the dictionary.
                let held: Vec<(String, Box<dyn DynObject>)> = cx
                    .bag
                    .objects()
                    .filter_map(|(_, known)| {
                        let reference = known.reference.as_ref()?;
                        let read = energyml_core::json::read_value(known.json.as_ref()?, &types).ok()?;
                        Some((reference.uuid.clone(), read.root))
                    })
                    .collect();
                let source = BagObjects { objects: held };
                let resolver = Resolver::new(&source);
                let kinds = resolve_kinds(&sheet, &resolver);
                let (wanted, wanted_word) = match params.by {
                    LinkBy::Key => (IDENTIFIER, "identifier"),
                    LinkBy::Rank => (EXPLICIT_INDEX, "explicit index"),
                };
                match kinds.get(at).and_then(|kind| kind.as_ref()) {
                    Some(resolution) if descends_from(resolution, wanted) => {}
                    Some(resolution) => {
                        return Err(Refusal::on(
                            "column",
                            format!(
                                "`{title}` has the kind `{}`, and a link {} takes a kind that is `{wanted_word}` or one of its descendants - set it first with `edit-object`: the kind is never changed in silence",
                                resolution.kind().title,
                                match params.by {
                                    LinkBy::Key => "by key",
                                    LinkBy::Rank => "by rank",
                                }
                            ),
                        ));
                    }
                    None => {
                        return Err(Refusal::on(
                            "column",
                            format!(
                                "the kind of `{title}` does not resolve, in the package or in the dictionary - set it first with `edit-object`: the kind is never changed in silence"
                            ),
                        ));
                    }
                }
                let key_count = sheet.columns.iter().filter(|column| column.key).count();
                let (field, rank) = if at < key_count {
                    ("KeyColumn", at)
                } else {
                    ("Column", at - key_count)
                };
                let own = sheet.columns[at]
                    .description
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("");
                (
                    format!("{field}[{rank}].Facet"),
                    Some(format!("{field}[{rank}].Description")),
                    Some(own.to_string()),
                )
            } else {
                if params.column.is_some() {
                    return Err(Refusal::on(
                        "column",
                        "a property has no column: leave it out - the link is written on the property itself",
                    ));
                }
                if params.by == LinkBy::Rank {
                    says.push(Note {
                        key: "by-ignored",
                        detail: "`by` has no effect on a property: its axes are named by the table's uri"
                            .to_string(),
                    });
                }
                ("Facet".to_string(), None, None)
            };

        // The facets as they stand, read from the JSON the object arrived in.
        let facets_json: Vec<Value> = facets_at(object_json, &facet_path);
        for facet in &facets_json {
            let kind = facet["Kind"].as_str().unwrap_or("");
            if !kind.eq_ignore_ascii_case("qualifier") {
                continue;
            }
            let value = facet["Facet"].as_str().unwrap_or("").trim();
            if value == uri {
                return Err(Refusal::on("object", "already links to it: nothing to write"));
            }
            return Err(Refusal::on(
                "object",
                format!(
                    "already carries the qualifier `{value}`: a link is one qualifier facet (S3), and a second would make it ambiguous"
                ),
            ));
        }
        let mut facets = facets_json;
        facets.push(json!({ "$type": "eml23.PropertyKindFacet", "Facet": uri, "Kind": "qualifier" }));

        let mut fields = vec![format!("{facet_path} = {}", Value::Array(facets))];
        // Convention A, derived: only for a link by key, only when the column
        // has no description.
        if let (Some(path), Some(own)) = (description_path, description) {
            if own.is_empty() {
                if params.by == LinkBy::Key {
                    let key = keys.first().copied().unwrap_or("");
                    let text = if uuid_of(&uri) == uuid_of(&object_ref.uri()) {
                        format!("key of this table (self reference): {uri}, KeyColumn `{key}`")
                    } else {
                        format!(
                            "key of the table `{}`: {uri}, KeyColumn `{key}`",
                            target_ref.title
                        )
                    };
                    fields.push(format!("{path} = {}", Value::String(text)));
                } else {
                    says.push(Note {
                        key: "no-description-for-rank",
                        detail: "a link by rank has no form in a description: the kind and the facet say it"
                            .to_string(),
                    });
                }
            } else {
                if own.starts_with("key of ")
                    && let Some(named) = described_uri(&own)
                    && uuid_of(named) != uuid_of(&uri)
                {
                    return Err(Refusal::on(
                        "column",
                        format!(
                            "its description already declares a link to `{named}`, and this one is to `{uri}`: two targets for one column - say one"
                        ),
                    ));
                }
                says.push(Note {
                    key: "description-kept",
                    detail: "the column's description is kept as it is".to_string(),
                });
            }
        }

        let mut plan = EditObject.plan(
            EditObjectInputs {
                object: object.clone(),
            },
            EditObjectParams {
                fields,
                copy: None,
                reference: None,
                reference_at: None,
            },
            cx,
        )?;
        let title = object_ref.title.clone();
        plan.activity.recipe = "table-link";
        plan.activity.title = format!("table-link → {title}");
        plan.activity.seed = format!("table-link:{}:{uri}", object_ref.uuid);
        plan.activity.parameters.push(Parameter::Object {
            title: "input".to_string(),
            reference: target_ref.dor(),
        });
        if let Some(column) = &params.column {
            plan.activity.parameters.push(Parameter::Text {
                title: "column".to_string(),
                value: column.clone(),
            });
        }
        plan.activity.parameters.push(Parameter::Text {
            title: "by".to_string(),
            value: params.by.word().to_string(),
        });
        plan.activity.parameters.push(Parameter::Text {
            title: "targetUri".to_string(),
            value: uri,
        });
        plan.says.extend(says);
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        vec![Slot::required(
            "targetUri",
            Need::Text,
            "the full uri of the table linked to, as written in the facet",
        )]
    }
}
