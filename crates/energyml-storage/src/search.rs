//! Finding objects by what a person remembers of them: a type, a title, a bare
//! uuid.
//!
//! The traits list objects ([`EnergymlStorage::resources`],
//! [`EnergymlStorage::resources_in`]) and read their citations
//! ([`EnergymlStorage::headers`]), but nothing combines the two: the command
//! line filters by hand, and a store that is not an EPC has no
//! `EpcStore::lookup_uuid` to turn a uuid into the full URI. These are free
//! functions over the traits, so that every face (command line, MCP server,
//! bridges) shares one implementation and no store has to learn a new filter.
//!
//! # Two spellings of "type", kept apart
//!
//! [`Query::types`] has the semantics of ETP's `dataObjectTypes`
//! ([`crate::context::type_matches`]): exact, or `family.*`, case sensitive.
//! [`Query::type_contains`] has those of [`crate::resource::type_matches`]: a
//! substring, case-insensitive. Two criteria under two names, because one name
//! for both is how they were confused before.
//!
//! # What is refused rather than answered empty
//!
//! A store that cannot list the dataspace asked for returns its own refusal
//! (`Unsupported`, `NotFound`), and a malformed type wildcard is a
//! [`Error::BadUri`]: "nothing matches" is only ever said by a store that
//! looked.

use crate::context::{self, Context};
use crate::error::{Error, Result};
use crate::resource::{self, ObjectSummary};
use crate::storage::EnergymlStorage;

/// What to look for. Every criterion is optional and they combine with AND.
///
/// Built with [`Query::new`] and the methods below; `#[non_exhaustive]`, so a
/// criterion added later is not a breaking change.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Query {
    /// Dataspace URI to search in; `None` = the store's whole listing (`resources()`).
    pub dataspace: Option<String>,
    /// ETP qualified types, exact or `family.*` (the semantics of `context::type_matches`).
    pub types: Vec<String>,
    /// Case-insensitive substring of the qualified type (the semantics of `resource::type_matches`).
    pub type_contains: Option<String>,
    /// Case-insensitive substring of the title (`resource::title_matches`).
    pub title: Option<String>,
}

impl Query {
    /// A query with no criterion: every object of the store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Searching in this dataspace (a dataspace URI, `eml:///` for the default one).
    pub fn in_dataspace(mut self, uri: impl Into<String>) -> Self {
        self.dataspace = Some(uri.into());
        self
    }

    /// Keeping only these qualified types, exact or `family.*`.
    pub fn with_types<I: IntoIterator<Item = S>, S: Into<String>>(mut self, types: I) -> Self {
        self.types = types.into_iter().map(Into::into).collect();
        self
    }

    /// Keeping only the types containing this text, ignoring case.
    pub fn type_containing(mut self, s: impl Into<String>) -> Self {
        self.type_contains = Some(s.into());
        self
    }

    /// Keeping only the objects whose title contains this text, ignoring case.
    pub fn titled(mut self, s: impl Into<String>) -> Self {
        self.title = Some(s.into());
        self
    }
}

/// What a search found and what it cost, for a caller that must report both.
///
/// [`find`] answers with the objects alone; this is the same search with its
/// bookkeeping kept, so that "scanned 2000, 3 headers unreadable" can be said
/// instead of left out.
#[derive(Debug)]
#[non_exhaustive]
pub struct Search {
    /// The objects matching, in the order the store listed them.
    pub objects: Vec<ObjectSummary>,
    /// How many objects the listing returned (after the store's own type
    /// filter, when a dataspace was named).
    pub listed: usize,
    /// How many headers were read. Zero unless a title was asked for.
    pub headers_read: usize,
    /// The objects whose header could not be read, with why. They are neither
    /// in [`Search::objects`] nor known not to match: a title that cannot be
    /// read cannot be compared.
    pub unreadable: Vec<(String, Error)>,
}

/// The objects matching `query`, in the order the store listed them.
///
/// The listing is [`EnergymlStorage::resources_in`] when a dataspace is named
/// (the type filter is then the store's own) and [`EnergymlStorage::resources`]
/// otherwise. `type_contains` and `title` are applied here.
///
/// **A title costs a header pass.** An [`ObjectSummary`] does not carry the
/// title, so when `title` is set the headers of every object that survived the
/// other criteria are read ([`EnergymlStorage::headers`]): a handful of bytes
/// per part on an EPC, a round trip per batch on a server. Narrow with
/// `types`, `type_contains` or a dataspace first on a large store.
///
/// **A header that cannot be read refuses the whole search**, with that
/// header's own error: an object whose title is unknown is neither a match nor
/// a non-match, and dropping it would answer "no such object" for one that may
/// exist. [`find_report`] keeps the unreadable ones aside instead.
pub async fn find<S: EnergymlStorage + Sync + ?Sized>(
    store: &S,
    query: &Query,
) -> Result<Vec<ObjectSummary>> {
    let mut search = find_report(store, query).await?;
    if search.unreadable.is_empty() {
        Ok(search.objects)
    } else {
        Err(search.unreadable.swap_remove(0).1)
    }
}

/// [`find`] with its bookkeeping, and with unreadable headers reported in
/// [`Search::unreadable`] rather than refusing the search.
pub async fn find_report<S: EnergymlStorage + Sync + ?Sized>(store: &S, query: &Query) -> Result<Search> {
    check_wildcards(&query.types)?;

    let listed = match &query.dataspace {
        // The store does the type filter: a server does it without sending
        // what is dropped.
        Some(dataspace) => {
            let context = Context::dataspace(dataspace.clone()).with_types(query.types.iter().cloned());
            store.resources_in(&context).await?.resources
        }
        None => {
            let all = store.resources().await?;
            if query.types.is_empty() {
                all
            } else {
                all.into_iter()
                    .filter(|summary| {
                        query
                            .types
                            .iter()
                            .any(|pattern| context::type_matches(pattern, &summary.qualified_type))
                    })
                    .collect()
            }
        }
    };
    let listed_count = listed.len();

    let candidates: Vec<ObjectSummary> = match &query.type_contains {
        Some(filter) => listed
            .into_iter()
            .filter(|summary| resource::type_matches(&summary.qualified_type, filter))
            .collect(),
        None => listed,
    };

    let Some(filter) = &query.title else {
        return Ok(Search {
            objects: candidates,
            listed: listed_count,
            headers_read: 0,
            unreadable: Vec::new(),
        });
    };

    let uris: Vec<String> = candidates.iter().map(|summary| summary.uri.clone()).collect();
    let headers = store.headers(&uris).await;
    if headers.len() != candidates.len() {
        return Err(Error::Remote {
            store: store.store_kind(),
            reason: format!(
                "asked for {} header(s), the store answered {}",
                candidates.len(),
                headers.len()
            ),
        });
    }
    let mut objects = Vec::new();
    let mut unreadable = Vec::new();
    for (summary, header) in candidates.into_iter().zip(headers) {
        match header {
            // No title is not "a title that does not match": the object is
            // simply not found by its title.
            Ok(header) => {
                if header
                    .title
                    .as_deref()
                    .is_some_and(|title| resource::title_matches(title, filter))
                {
                    objects.push(summary);
                }
            }
            Err(error) => unreadable.push((summary.uri, error)),
        }
    }
    Ok(Search {
        objects,
        listed: listed_count,
        headers_read: uris.len(),
        unreadable,
    })
}

/// A bare uuid (admitted at the entry only) turned at once into the full URI.
///
/// `qualified_type` narrows when several objects share the uuid (versions, or a
/// store that holds the same uuid under two types). No match: `Error::NotFound`.
/// Several: [`Error::Ambiguous`] with every candidate URI.
///
/// The uuid is compared ignoring ASCII case and nothing else is stripped: the
/// same rule as `EpcStore::lookup_uuid`, which lowercases and looks the text up
/// as it stands, so a uuid in braces is not found. The type is compared as
/// written (qualified types are case sensitive).
///
/// Reads the whole listing ([`EnergymlStorage::resources`]): identity only, no
/// object is opened.
pub async fn resolve_uuid<S: EnergymlStorage + Sync + ?Sized>(
    store: &S,
    uuid: &str,
    qualified_type: Option<&str>,
) -> Result<String> {
    if uuid.is_empty() {
        return Err(Error::BadUri {
            uri: String::new(),
            reason: "an empty uuid names no object".into(),
        });
    }
    let given = match qualified_type {
        Some(qualified_type) => format!("{qualified_type}({uuid})"),
        None => uuid.to_string(),
    };
    let mut matching: Vec<String> = store
        .resources()
        .await?
        .into_iter()
        .filter(|summary| summary.uuid.eq_ignore_ascii_case(uuid))
        .filter(|summary| qualified_type.is_none_or(|wanted| summary.qualified_type == wanted))
        .map(|summary| summary.uri)
        .collect();
    match matching.len() {
        0 => Err(Error::NotFound(given)),
        1 => Ok(matching.swap_remove(0)),
        _ => Err(Error::Ambiguous {
            uri: given,
            candidates: matching,
        }),
    }
}

/// The wildcard rule of [`context::discover`], applied to a search that does
/// not go through it (no dataspace named): a wildcard is a schema version
/// followed by `.*`. Left alone, `resqml22.*x` would match nothing and the
/// search would answer "none" to a question that was never well formed.
fn check_wildcards(types: &[String]) -> Result<()> {
    match types
        .iter()
        .find(|t| t.contains('*') && (!t.ends_with(".*") || t[..t.len() - 1].contains('*')))
    {
        Some(odd) => Err(Error::BadUri {
            uri: odd.clone(),
            reason: "a type wildcard is a schema version followed by `.*` (ETP 1.2 §23.34.15)".into(),
        }),
        None => Ok(()),
    }
}
