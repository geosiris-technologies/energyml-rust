//! Choosing rules and writing them out — what a face (the command line today,
//! wasm and Python later) needs so that none of them holds logic of its own.
//!
//! The JSON is written by hand: this crate has no runtime dependency, wasm
//! included, and the document is flat enough that a writer is shorter than a
//! dependency.

use crate::{
    Anchor, AnchorKind, Localised, RULESET_VERSION, Rule, Status, Text, all, locales, text_in, theme_title,
    themes,
};

/// Which rules to keep. Every field that is set must match.
#[derive(Clone, Debug, Default)]
pub struct Filter<'a> {
    /// One theme.
    pub theme: Option<&'a str>,
    /// One id (or one of its aliases).
    pub id: Option<&'a str>,
    /// Rules anchored at a place: `(kind, key)`, see [`parse_anchor`].
    pub anchor: Option<(AnchorKind, &'a str)>,
    /// One status.
    pub status: Option<Status>,
}

/// Reads `kind:key` — `type:Attachment`, `type:Attachment.Md`,
/// `view:object.colour-stack`, `finding:house/…`, `operation:attach`,
/// `diagnostic:…`. The key may itself hold a colon (`view:create.op:crs`): only
/// the first one separates.
pub fn parse_anchor(text: &str) -> Result<(AnchorKind, &str), String> {
    let (kind, key) = text.split_once(':').ok_or_else(|| {
        format!("`{text}` is not `kind:key` (kinds: type, view, finding, operation, diagnostic)")
    })?;
    let kind = AnchorKind::parse(kind).ok_or_else(|| {
        format!("`{kind}` is not an anchor kind (type, view, finding, operation, diagnostic)")
    })?;
    if key.is_empty() {
        Err(format!("`{text}` has an empty key"))
    } else {
        Ok((kind, key))
    }
}

/// Reads a status word.
pub fn parse_status(word: &str) -> Result<Status, String> {
    [Status::Active, Status::Designed, Status::Superseded]
        .into_iter()
        .find(|s| s.as_str() == word)
        .ok_or_else(|| format!("`{word}` is not a status (active, designed, superseded)"))
}

/// The rules the filter keeps, in registry order.
pub fn select(filter: &Filter<'_>) -> Vec<&'static Rule> {
    all()
        .iter()
        .filter(|r| filter.theme.is_none_or(|t| r.theme == t))
        .filter(|r| filter.id.is_none_or(|i| r.id == i || r.aliases.contains(&i)))
        .filter(|r| filter.status.is_none_or(|s| r.status == s))
        .filter(|r| {
            filter
                .anchor
                .is_none_or(|(kind, key)| r.at.iter().any(|a: &Anchor| a.kind == kind && a.key == key))
        })
        .collect()
}

fn esc(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn opt(s: Option<&str>, out: &mut String) {
    match s {
        Some(s) => esc(s, out),
        None => out.push_str("null"),
    }
}

fn list<T>(items: &[T], out: &mut String, mut one: impl FnMut(&T, &mut String)) {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        one(item, out);
    }
    out.push(']');
}

/// A text object: the words of every language that has them. English is
/// always there; another language appears **only when it is translated** — the
/// absence is the information.
fn texts(english: &str, other: impl Iterator<Item = (&'static str, Option<&'static str>)>, out: &mut String) {
    out.push_str("{\"en\":");
    esc(english, out);
    for (lang, value) in other {
        if let Some(value) = value {
            out.push(',');
            esc(lang, out);
            out.push(':');
            esc(value, out);
        }
    }
    out.push('}');
}

/// The JSON document of the design, for `rules`:
/// `{version, locales, themes, rules}`, every language that exists, text
/// never duplicated across rules. Stable: the same registry gives the same
/// bytes.
pub fn to_json(rules: &[&Rule]) -> String {
    let langs: Vec<&'static str> = locales().into_iter().filter(|l| *l != "en").collect();
    let mut out = String::from("{\"version\":");
    esc(RULESET_VERSION, &mut out);
    out.push_str(",\"locales\":");
    list(&locales(), &mut out, |l, o| esc(l, o));
    out.push_str(",\"themes\":");
    list(themes(), &mut out, |t, o| {
        o.push_str("{\"id\":");
        esc(t.id, o);
        o.push_str(",\"title\":");
        texts(
            t.title,
            langs.iter().map(|l| {
                (
                    *l,
                    Some(theme_title(t, l)).filter(|x| x.lang == *l).map(|x| x.value),
                )
            }),
            o,
        );
        o.push('}');
    });
    out.push_str(",\"rules\":");
    list(rules, &mut out, |r, o| {
        let tr = |pick: fn(&Localised) -> Text| {
            langs
                .iter()
                .map(move |l| {
                    (
                        *l,
                        Some(pick(&text_in(r, l)))
                            .filter(|x| x.lang == *l)
                            .map(|x| x.value),
                    )
                })
                .collect::<Vec<_>>()
        };
        o.push_str("{\"id\":");
        esc(r.id, o);
        o.push_str(",\"theme\":");
        esc(r.theme, o);
        o.push_str(",\"status\":");
        esc(r.status.as_str(), o);
        o.push_str(",\"stance\":");
        esc(r.stance.as_str(), o);
        o.push_str(&format!(",\"level\":{},\"since\":", r.level));
        esc(r.since, o);
        o.push_str(",\"title\":");
        texts(r.title, tr(|l| l.title).into_iter(), o);
        o.push_str(",\"text\":");
        texts(r.text, tr(|l| l.text).into_iter(), o);
        o.push_str(",\"why\":");
        texts(r.why, tr(|l| l.why).into_iter(), o);
        o.push_str(",\"rulings\":");
        list(r.rulings, o, |g, o| {
            o.push_str("{\"by\":");
            esc(g.by, o);
            o.push_str(",\"on\":");
            esc(g.on, o);
            o.push_str(",\"quote\":");
            opt(g.quote, o);
            o.push('}');
        });
        o.push_str(",\"refs\":");
        list(r.refs, o, |f, o| {
            o.push_str("{\"kind\":");
            esc(f.kind.as_str(), o);
            o.push_str(",\"doc\":");
            opt(f.doc, o);
            o.push_str(",\"path\":");
            opt(f.path, o);
            o.push_str(",\"at\":");
            opt(f.at, o);
            o.push_str(",\"note\":");
            opt(f.note, o);
            o.push('}');
        });
        o.push_str(",\"at\":");
        list(r.at, o, |a, o| {
            o.push_str("{\"type\":");
            esc(a.kind.as_str(), o);
            o.push_str(",\"key\":");
            esc(a.key, o);
            o.push('}');
        });
        o.push_str(",\"supersedes\":");
        list(r.supersedes, o, |s, o| esc(s, o));
        o.push_str(",\"applies\":");
        list(r.applies, o, |s, o| esc(s, o));
        o.push_str(",\"switch\":");
        opt(r.switch, o);
        o.push('}');
    });
    out.push('}');
    out
}
