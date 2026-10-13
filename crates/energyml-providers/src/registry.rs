//! The list of providers — a registry, not a `match`.
//!
//! The repository's rule wherever a list grows: a trait, a registry, the
//! shipped providers pushed by default, and **the caller's tried first** so that
//! a shipped one can be replaced and not only completed (ADR-0006). It is the
//! shape of the exporters and of the array backends; it will be that of an OGC
//! API Features or a STAC the day they arrive.

use serde::{Deserialize, Serialize};

use crate::{About, Collection, Provider};

/// What this build knows how to query.
#[derive(Default)]
pub struct Providers {
    added: Vec<Box<dyn Provider>>,
    builtin: Vec<Box<dyn Provider>>,
}

impl Providers {
    /// No provider. For a caller who brings only its own.
    pub fn none() -> Self {
        Providers::default()
    }

    /// The ones this build ships.
    pub fn builtin() -> Self {
        Providers {
            added: Vec::new(),
            builtin: vec![Box::new(crate::brgm::bss())],
        }
    }

    /// Add one's own, **tried before** the shipped ones.
    pub fn push(&mut self, provider: Box<dyn Provider>) -> &mut Self {
        self.added.push(provider);
        self
    }

    /// All of them, the added ones first.
    pub fn all(&self) -> impl Iterator<Item = &dyn Provider> {
        self.added
            .iter()
            .chain(self.builtin.iter())
            .map(|one| one.as_ref())
    }

    /// The one with this name.
    ///
    /// The refusal **lists what exists**: somebody who mistyped a name needs the
    /// names.
    pub fn get(&self, name: &str) -> crate::Result<&dyn Provider> {
        self.all().find(|one| one.name() == name).ok_or_else(|| {
            crate::Error::Refused(format!(
                "`{name}` is not a provider this build knows — it has {}",
                self.all().map(|one| one.name()).collect::<Vec<_>>().join(", ")
            ))
        })
    }
}

/// Everything this build knows how to query, **in one value**.
///
/// The single door to the outside. The front end does not read the providers one by
/// one: it asks for this once, and draws from it its list of APIs, the
/// collections of each, and the form of each collection. A provider added to
/// [`Providers::builtin`] appears here **without a line changing elsewhere** —
/// the property this crate exists to hold, and which
/// `an_added_provider_reaches_the_catalogue_without_touching_anything` checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    /// The providers, the added ones first — the registry's order.
    pub providers: Vec<Declared>,
}

/// A provider as it presents itself to somebody who does not know it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Declared {
    /// The short name, the one that is typed and sent back.
    pub name: String,
    /// The sentence of a list.
    pub describes: String,
    /// The address and the credit, **together**.
    pub about: About,
    /// What it offers.
    pub collections: Vec<Collection>,
}

impl Providers {
    /// What this registry offers, serialisable.
    pub fn catalogue(&self) -> Catalogue {
        Catalogue {
            providers: self
                .all()
                .map(|one| Declared {
                    name: one.name().to_string(),
                    describes: one.describe().to_string(),
                    about: one.about(),
                    collections: one.collections(),
                })
                .collect(),
        }
    }
}
