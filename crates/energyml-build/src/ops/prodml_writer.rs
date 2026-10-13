//! What the PRODML operations that write an invented object share - the
//! citation that says it is synthetic and where its values come from, the
//! `synthetic = true` marker, the derived uuids, the UTC instants.
//!
//! `prodml-doublet` wrote these as closures of its own plan; `fluid-sample`,
//! `dts-installation` and `product-flow-model` write the same three things on
//! every object, so they are here once - with the doublet's helpers
//! ([`super::prodml_doublet::synthetic_by`], `titled`, `wellbore_because`)
//! reused rather than copied.
//!
//! The rules: an invented value says so - `ExtensionNameValue` `synthetic = true` and a sentence in
//! `Citation/Description` on every object invented - and no constant
//! without a source: the source is required, and a blank one is refused.

use energyml_eml_2_3 as eml;
use energyml_storage::epc::core_props::unix_seconds;

use crate::operation::{Identity, NewObject, Refusal, When};
use crate::typed::Context;

/// Writes the objects of one plan, each cited, sourced and marked synthetic.
pub(crate) struct Writer<'a> {
    identity: &'a Identity,
    when: &'a When,
    operation: &'static str,
    source: String,
    rank: usize,
    /// What the plan will write, in order.
    pub objects: Vec<NewObject>,
}

impl<'a> Writer<'a> {
    /// A writer for `operation`, whose values come from `source` - refused
    /// when blank (ADR-0019).
    pub fn new(cx: &'a Context<'a>, operation: &'static str, source: &str) -> Result<Self, Refusal> {
        let source = source.trim();
        if source.is_empty() {
            return Err(Refusal::on(
                "source",
                "is empty: every value this operation writes is invented, and an invented value states where it comes from",
            ));
        }
        Ok(Writer {
            identity: cx.identity,
            when: cx.when,
            operation,
            source: source.to_string(),
            rank: 0,
            objects: Vec::new(),
        })
    }

    /// The uuid of the next object, from what it is.
    pub fn uuid(&mut self, seed: &str) -> Result<String, Refusal> {
        let uuid = self
            .identity
            .uuid(&format!("{}:{seed}", self.operation), self.rank)?;
        self.rank += 1;
        Ok(uuid)
    }

    /// A citation whose description says the object is synthetic, what it
    /// is, and where its values come from - refused past the 2000 characters
    /// a `Citation/Description` holds.
    pub fn citation(&self, title: &str, what: &str) -> Result<eml::Citation, Refusal> {
        let description = format!("Synthetic - {what}. Source: {}", self.source);
        let length = description.chars().count();
        if length > 2000 {
            return Err(Refusal::on(
                "source",
                format!(
                    "makes a description of {length} characters, and a Citation/Description holds 2000 - cite the document shorter"
                ),
            ));
        }
        Ok(eml::Citation {
            title: title.to_string(),
            originator: self.when.originator.clone(),
            creation: self.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            description: Some(description),
            ..Default::default()
        })
    }

    /// `synthetic = true`, naming this operation.
    pub fn synthetic(&self, what: &str) -> eml::ExtensionNameValue {
        super::prodml_doublet::synthetic_by(self.operation, what)
    }

    /// One object more, written as its generated type encodes itself.
    pub fn push(
        &mut self,
        uuid: &str,
        qualified_type: &str,
        role: &str,
        title: &str,
        object: &dyn energyml_core::value::Reflect,
    ) {
        self.objects.push(NewObject {
            uuid: uuid.to_string(),
            qualified_type: qualified_type.to_string(),
            role: role.to_string(),
            title: title.to_string(),
            json: energyml_core::json::write_value(object),
        });
    }

    /// The seed of the activity: every object this run wrote.
    pub fn activity_seed(&self) -> String {
        format!(
            "{}:{}",
            self.operation,
            self.objects
                .iter()
                .map(|one| one.uuid.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

/// A UTC date-time as `2025-06-02T00:00:00Z`, or a refusal naming the slot:
/// the text as given, and its seconds.
pub(crate) fn instant(slot: &str, text: &str) -> Result<(String, i64), Refusal> {
    let text = text.trim();
    match unix_seconds(text) {
        Some(seconds) => Ok((text.to_string(), seconds)),
        None => Err(Refusal::on(
            slot,
            format!(
                "`{text}` is not a UTC date-time as `2025-06-02T00:00:00Z`: it is refused rather than read some other way"
            ),
        )),
    }
}
