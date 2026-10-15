//! What every tool shares: reading arguments, resolving an identity, paging,
//! saying that an answer was cut.
//!
//! None of this is logic about EnergyML. It is the shape of an MCP answer: the
//! same `total`, `returned`, `truncated`, `next_cursor` on every list, so that a
//! model learns them once.

use std::collections::HashMap;

use energyml_core::identifiers::EtpUri;
use energyml_storage::EnergymlStorage;
use energyml_storage::resource::ObjectSummary;
use energyml_storage::search::resolve_uuid;
use serde_json::{Map, Value, json};

use crate::refusal::{Refusal, RefusalCode};
use crate::tool::ToolContext;

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// The arguments of a call, checked against the schema the tool published.
///
/// The server is the one that validates (the specification says it MUST), and an
/// unknown key is refused by name, not ignored: a model that misspells `limit`
/// must be told, not served the default.
pub(crate) struct Args {
    map: Map<String, Value>,
}

impl Args {
    /// `null` is an empty object (some clients send it for a tool without
    /// arguments); anything but an object is refused, as is any key not in
    /// `allowed`.
    pub(crate) fn parse(value: Value, allowed: &[&str]) -> Result<Self, Refusal> {
        let map = match value {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            _ => return Err(Refusal::bad_arguments("the arguments must be a JSON object")),
        };
        if let Some(unknown) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(
                Refusal::bad_arguments("an argument is not part of this tool's schema")
                    .with_hint(format!("the arguments of this tool are: {}", allowed.join(", ")))
                    .with_detail(unknown),
            );
        }
        Ok(Self { map })
    }

    fn wrong(key: &str, rule: &str) -> Refusal {
        Refusal::bad_arguments(format!("argument `{key}` {rule}"))
    }

    /// An optional, non-empty string.
    pub(crate) fn string(&self, key: &str) -> Result<Option<String>, Refusal> {
        match self.map.get(key) {
            None => Ok(None),
            Some(Value::String(text)) if !text.is_empty() => Ok(Some(text.clone())),
            Some(Value::String(_)) => Err(Self::wrong(key, "must not be empty")),
            Some(_) => Err(Self::wrong(key, "must be a string")),
        }
    }

    /// An optional integer in `min..=max`. Out of range is refused, not clamped:
    /// the schema said the range.
    pub(crate) fn integer(&self, key: &str, min: i64, max: i64) -> Result<Option<i64>, Refusal> {
        match self.map.get(key) {
            None => Ok(None),
            Some(value) => match value.as_i64() {
                Some(n) if (min..=max).contains(&n) => Ok(Some(n)),
                Some(_) => Err(Self::wrong(key, &format!("must be between {min} and {max}"))),
                None => Err(Self::wrong(key, "must be an integer")),
            },
        }
    }

    /// [`Args::integer`] as a `usize`; `min` is at least 0.
    pub(crate) fn count(&self, key: &str, min: usize, max: usize) -> Result<Option<usize>, Refusal> {
        Ok(self.integer(key, min as i64, max as i64)?.map(|n| n as usize))
    }

    /// An optional boolean.
    pub(crate) fn boolean(&self, key: &str) -> Result<Option<bool>, Refusal> {
        match self.map.get(key) {
            None => Ok(None),
            Some(Value::Bool(flag)) => Ok(Some(*flag)),
            Some(_) => Err(Self::wrong(key, "must be a boolean")),
        }
    }

    /// An optional word among `choices`.
    pub(crate) fn one_of(&self, key: &str, choices: &[&str]) -> Result<Option<String>, Refusal> {
        match self.string(key)? {
            None => Ok(None),
            Some(word) if choices.contains(&word.as_str()) => Ok(Some(word)),
            Some(_) => Err(Self::wrong(
                key,
                &format!("must be one of: {}", choices.join(", ")),
            )),
        }
    }

    /// An optional array of non-empty strings.
    pub(crate) fn strings(&self, key: &str) -> Result<Option<Vec<String>>, Refusal> {
        match self.map.get(key) {
            None => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Value::String(text) if !text.is_empty() => Ok(text.clone()),
                    _ => Err(Self::wrong(key, "must be an array of non-empty strings")),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(Self::wrong(key, "must be an array of strings")),
        }
    }

    /// An optional array of integers, each at least `min`.
    pub(crate) fn integers(&self, key: &str, min: i64) -> Result<Option<Vec<i64>>, Refusal> {
        match self.map.get(key) {
            None => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item.as_i64() {
                    Some(n) if n >= min => Ok(n),
                    _ => Err(Self::wrong(
                        key,
                        &format!("must be an array of integers, each at least {min}"),
                    )),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(Self::wrong(key, "must be an array of integers")),
        }
    }
}

// ---------------------------------------------------------------------------
// Identity: the one door
// ---------------------------------------------------------------------------

/// `EtpUri::parse` as a refusal: the one door for every `uri` argument.
///
/// The string is kept as given (the store parses it again, and a canonical form
/// is the store's to produce); what this guarantees is that nothing that does
/// not parse goes any further.
pub(crate) fn parse_uri(uri: &str) -> Result<EtpUri, Refusal> {
    EtpUri::parse(uri).map_err(|error| {
        Refusal::new(RefusalCode::BadUri, "the value is not an ETP URI")
            .with_hint("copy a URI exactly as another tool returned it, for example eml:///dataspace('x')/resqml22.BoundaryFeature(<uuid>)")
            .with_detail(format!("{uri}: {error}"))
    })
}

/// The URI of the object an `uri` argument names, or - at the entry only - the
/// URI an `uuid` argument resolves to through `resolve_uuid`. From here on the
/// full URI is all that circulates.
pub(crate) async fn object_uri(
    ctx: &ToolContext<'_>,
    args: &Args,
    accept_uuid: bool,
) -> Result<String, Refusal> {
    let uri = args.string("uri")?;
    let uuid = if accept_uuid { args.string("uuid")? } else { None };
    let qualified_type = if accept_uuid { args.string("type")? } else { None };
    match (uri, uuid) {
        (Some(_), Some(_)) => Err(Refusal::bad_arguments("give `uri` or `uuid`, not both")),
        (None, None) => Err(Refusal::bad_arguments(if accept_uuid {
            "give `uri` (or, if you have none, `uuid`)"
        } else {
            "argument `uri` is required"
        })),
        (Some(uri), None) => {
            if qualified_type.is_some() {
                return Err(Refusal::bad_arguments("`type` goes with `uuid`, not with `uri`"));
            }
            let parsed = parse_uri(&uri)?;
            if parsed.is_dataspace() {
                return Err(
                    Refusal::new(RefusalCode::BadUri, "the URI names a dataspace, not an object")
                        .with_hint("pass the URI of an object, as energyml_list_resources returns it")
                        .with_detail(uri),
                );
            }
            Ok(uri)
        }
        (None, Some(uuid)) => resolve_uuid(ctx.store, &uuid, qualified_type.as_deref())
            .await
            .map_err(Refusal::from),
    }
}

// ---------------------------------------------------------------------------
// Paging
// ---------------------------------------------------------------------------

/// How a list is cut.
pub(crate) struct Paging {
    /// First item to return.
    pub(crate) offset: usize,
    /// How many items at most (already lowered to the server's ceiling).
    pub(crate) limit: usize,
    /// The server's ceiling, to tell "you asked for this" from "the server caps this".
    pub(crate) server_cap: usize,
    /// Bytes the items may take.
    pub(crate) byte_budget: usize,
    /// Whether this list has a cursor.
    pub(crate) cursor: bool,
    /// How to get more when it has none.
    pub(crate) more: &'static str,
}

/// One page of a list, and the words that say it was cut.
pub(crate) struct Page {
    items: Vec<Value>,
    total: usize,
    truncated: bool,
    truncation: Option<Value>,
    next_cursor: Option<String>,
}

/// The opaque cursor of an offset. Opaque to the model; stateless on the
/// server, so a store that changes between two pages can shift them.
fn encode_cursor(offset: usize) -> String {
    format!("o:{offset}")
}

/// The offset a cursor stands for; refused if it is not one this server wrote
/// or points past the end.
pub(crate) fn decode_cursor(cursor: Option<String>, total: usize) -> Result<usize, Refusal> {
    let Some(cursor) = cursor else { return Ok(0) };
    let offset = cursor
        .strip_prefix("o:")
        .and_then(|digits| digits.parse::<usize>().ok())
        .ok_or_else(|| Refusal::bad_arguments("argument `cursor` is not a cursor this server returned"))?;
    if offset > total {
        return Err(
            Refusal::bad_arguments("argument `cursor` points past the end of the list")
                .with_hint("start again without a cursor"),
        );
    }
    Ok(offset)
}

/// Cut `items[offset..]` by count and by bytes.
pub(crate) fn paginate(items: &[Value], paging: &Paging) -> Page {
    paginate_slice(items, 0, items.len(), paging)
}

/// [`paginate`] for a list whose later items were not built: `items` holds the
/// ones from global index `base`, and `total` is the length of the whole list.
/// `paging.offset` is global and must not be below `base`.
pub(crate) fn paginate_slice(items: &[Value], base: usize, total: usize, paging: &Paging) -> Page {
    let mut taken = Vec::new();
    let mut spent = 0usize;
    let mut by_bytes = false;
    for item in items.iter().skip(paging.offset.saturating_sub(base)) {
        if taken.len() >= paging.limit {
            break;
        }
        // One byte for the comma.
        let size = item.to_string().len() + 1;
        if spent + size > paging.byte_budget {
            by_bytes = true;
            break;
        }
        spent += size;
        taken.push(item.clone());
    }
    let end = paging.offset + taken.len();
    let truncated = end < total;
    let (truncation, next_cursor) = if truncated {
        let (reason, limit) = if by_bytes {
            ("response_bytes", paging.byte_budget)
        } else if paging.limit >= paging.server_cap {
            ("server_max_list", paging.server_cap)
        } else {
            ("limit", paging.limit)
        };
        let how = if paging.cursor {
            "pass `next_cursor` as `cursor` to get the next page".to_string()
        } else {
            paging.more.to_string()
        };
        (
            Some(json!({ "reason": reason, "limit": limit, "how_to_get_more": how })),
            paging.cursor.then(|| encode_cursor(end)),
        )
    } else {
        (None, None)
    };
    Page {
        items: taken,
        total,
        truncated,
        truncation,
        next_cursor,
    }
}

impl Page {
    /// The items of the page, for a caller that enriches them after the cut.
    pub(crate) fn items_mut(&mut self) -> &mut [Value] {
        &mut self.items
    }

    /// Write `key`, `total`, `returned`, `truncated`, and when true `truncation`
    /// and `next_cursor`, into an answer.
    pub(crate) fn put(self, body: &mut Map<String, Value>, key: &str) {
        let returned = self.items.len();
        body.insert(key.into(), Value::Array(self.items));
        body.insert("total".into(), json!(self.total));
        body.insert("returned".into(), json!(returned));
        put_truncation(body, self.truncated, self.truncation);
        if let Some(cursor) = self.next_cursor {
            body.insert("next_cursor".into(), json!(cursor));
        }
    }

    /// Like [`Page::put`] for the second list of an answer: its own `total` and
    /// `returned` under prefixed names, and its truncation merged in.
    pub(crate) fn put_secondary(self, body: &mut Map<String, Value>, key: &str) {
        let returned = self.items.len();
        body.insert(key.into(), Value::Array(self.items));
        body.insert(format!("{key}_total"), json!(self.total));
        body.insert(format!("{key}_returned"), json!(returned));
        if self.truncated {
            body.insert("truncated".into(), json!(true));
            if let Some(mut truncation) = self.truncation {
                // Two lists may be cut; each says which it is. The first to be
                // reported keeps `truncation`, the other goes under its list.
                if let Some(fields) = truncation.as_object_mut() {
                    fields.insert("list".into(), json!(key));
                }
                if body.contains_key("truncation") {
                    body.insert(format!("{key}_truncation"), truncation);
                } else {
                    body.insert("truncation".into(), truncation);
                }
            }
        }
    }
}

/// `truncated`, and `truncation` when it is true. Every answer calls this.
pub(crate) fn put_truncation(body: &mut Map<String, Value>, truncated: bool, truncation: Option<Value>) {
    body.insert("truncated".into(), json!(truncated));
    if truncated && let Some(truncation) = truncation {
        body.insert("truncation".into(), truncation);
    }
}

// ---------------------------------------------------------------------------
// Shaping
// ---------------------------------------------------------------------------

/// A number as JSON. JSON has no NaN or infinity, and `json!` would turn them
/// into `null` without a word: they are written as the strings `"NaN"`,
/// `"inf"`, `"-inf"` instead.
pub(crate) fn number(x: f64) -> Value {
    if x.is_finite() {
        json!(x)
    } else if x.is_nan() {
        json!("NaN")
    } else if x > 0.0 {
        json!("inf")
    } else {
        json!("-inf")
    }
}

/// An optional number.
pub(crate) fn optional_number(x: Option<f64>) -> Value {
    x.map_or(Value::Null, number)
}

/// What a listing shows of an object: the three facts that identify it.
pub(crate) fn summary_value(summary: &ObjectSummary) -> Map<String, Value> {
    let mut item = Map::new();
    item.insert("uri".into(), json!(summary.uri));
    item.insert("uuid".into(), json!(summary.uuid));
    item.insert("qualified_type".into(), json!(summary.qualified_type));
    item
}

/// Titles of these objects, read from their headers, and how many headers could
/// not be read (said, never silent).
pub(crate) async fn titles_of(
    store: &(dyn EnergymlStorage + Send + Sync),
    uris: &[String],
) -> (HashMap<String, String>, usize) {
    let mut titles = HashMap::new();
    let mut unreadable = 0;
    if uris.is_empty() {
        return (titles, unreadable);
    }
    for (uri, header) in uris.iter().zip(store.headers(uris).await) {
        match header {
            Ok(header) => {
                if let Some(title) = header.title {
                    titles.insert(uri.clone(), title);
                }
            }
            Err(_) => unreadable += 1,
        }
    }
    (titles, unreadable)
}

/// The locations of the arrays an object points at, in the order the store
/// gives them: the `index` the tools hand to the model.
pub(crate) async fn array_locations(
    ctx: &ToolContext<'_>,
    uri: &str,
) -> Result<Vec<energyml_storage::ArrayLocation>, Refusal> {
    let object = ctx.store.get_object(uri).await?;
    Ok(ctx.store.arrays_of(object.as_reflect()))
}

/// The number of elements a shape holds; saturating, never negative.
pub(crate) fn element_count(dimensions: &[i64]) -> u64 {
    dimensions
        .iter()
        .fold(1u64, |acc, dim| acc.saturating_mul((*dim).max(0) as u64))
}

/// `"all"`, or the ML prefixes the store states edge kinds for
/// (`Capabilities::edge_kind_coverage`).
pub(crate) fn coverage_value(coverage: &energyml_storage::EdgeKindCoverage) -> serde_json::Value {
    match coverage {
        energyml_storage::EdgeKindCoverage::Mls(prefixes) => serde_json::json!(prefixes),
        // `EdgeKindCoverage` is `#[non_exhaustive]`; what is not a list of MLs is "all".
        _ => serde_json::json!("all"),
    }
}
