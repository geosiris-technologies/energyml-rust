//! The house rules: what this library decides where the specification is
//! silent, ambiguous or only interpreted.
//!
//! A **house rule** is a decision of this software that changes what a user
//! sees, reads or is refused, and that no specification dictates word for word.
//! This crate is the registry of them — a leaf with **no runtime dependency**
//! (wasm included), so that the crates below `energyml-domain` can cite a rule
//! by id and a face can list them all without a front end.
//!
//! The rules are written in `rules/*.toml` and compiled by `build.rs` into
//! statics: [`id`] holds one constant per rule, so a misspelt id **does not
//! compile**, and nothing is parsed when a program runs. English is the source
//! and is always present; French (`rules/fr/*.toml`) is optional per rule and a
//! rule without it answers in English **and says so** ([`Localised::fallback`]).
//!
//! References — a design document, a page and paragraph of a specification, a
//! piece of code — are quoted strings. Nothing here opens a file at run time.
//!
//! Design: ADR-0005.

#![forbid(unsafe_code)]

pub mod select;

/// Where a rule stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// Decided and coded.
    Active,
    /// Decided, not yet coded: the rule is the design, said as such.
    Designed,
    /// Written before, kept for the history; a later rule `supersedes` it.
    Superseded,
}

impl Status {
    /// The word the files and the JSON use.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Designed => "designed",
            Status::Superseded => "superseded",
        }
    }
}

/// What the specification does about the question a rule answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stance {
    /// It says nothing.
    Silent,
    /// It allows two readings; one was taken.
    Ambiguous,
    /// It says about this much, and the rule says the rest.
    Interpreted,
    /// The rule goes beyond the standard: the file is no longer strict standard.
    Extends,
    /// It restates a cited text and makes it executable.
    Restates,
}

impl Stance {
    /// The word the files and the JSON use.
    pub fn as_str(self) -> &'static str {
        match self {
            Stance::Silent => "silent",
            Stance::Ambiguous => "ambiguous",
            Stance::Interpreted => "interpreted",
            Stance::Extends => "extends",
            Stance::Restates => "restates",
        }
    }
}

/// The kind of a reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefKind {
    /// A specification: `doc` and `at` (section, page).
    Spec,
    /// A design document of this repository: `path`.
    Design,
    /// A piece of code: `path`.
    Code,
    /// A user guide: `path`.
    Guide,
}

impl RefKind {
    /// The word the files and the JSON use.
    pub fn as_str(self) -> &'static str {
        match self {
            RefKind::Spec => "spec",
            RefKind::Design => "design",
            RefKind::Code => "code",
            RefKind::Guide => "guide",
        }
    }
}

/// The kind of an anchor: where a rule shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnchorKind {
    /// A schema type, `typeKey` or `typeKey.field` — never a qualified name,
    /// because a rule holds for every release that carries the type.
    Type,
    /// A view of a front end, from the closed list of `build/loader.rs`.
    View,
    /// The id of a check.
    Finding,
    /// The id of an operation of `energyml-build`.
    Operation,
    /// A diagnostic's key.
    Diagnostic,
}

impl AnchorKind {
    /// The word the files and the JSON use.
    pub fn as_str(self) -> &'static str {
        match self {
            AnchorKind::Type => "type",
            AnchorKind::View => "view",
            AnchorKind::Finding => "finding",
            AnchorKind::Operation => "operation",
            AnchorKind::Diagnostic => "diagnostic",
        }
    }

    /// The reverse of [`AnchorKind::as_str`].
    pub fn parse(word: &str) -> Option<Self> {
        [
            AnchorKind::Type,
            AnchorKind::View,
            AnchorKind::Finding,
            AnchorKind::Operation,
            AnchorKind::Diagnostic,
        ]
        .into_iter()
        .find(|k| k.as_str() == word)
    }
}

/// Where a rule shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// What `key` names.
    pub kind: AnchorKind,
    /// The type, view, check id, operation or diagnostic key.
    pub key: &'static str,
}

/// Who decided a rule, and when. Named `Ruling` and not `Decision`, which in
/// `energyml-domain::color` is the result of a walk, not the origin of a rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ruling {
    /// Who.
    pub by: &'static str,
    /// When, `YYYY-MM-DD`.
    pub on: &'static str,
    /// The words, in the language they were said — a citation is not translated.
    pub quote: Option<&'static str>,
}

/// A reference, quoted and never read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ref {
    /// Which sort.
    pub kind: RefKind,
    /// The specification's name (a `spec` reference), from a closed list.
    pub doc: Option<&'static str>,
    /// A repository-relative path (every other kind).
    pub path: Option<&'static str>,
    /// The place inside: a section, a page, a symbol.
    pub at: Option<&'static str>,
    /// What to know about the reference — for a rule saying the specification
    /// is silent, where we looked.
    pub note: Option<&'static str>,
}

/// One house rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    /// `house/<theme>/<name>`; the two ids published before that shape are flat.
    pub id: &'static str,
    /// The theme, a field and not a piece of the id to parse.
    pub theme: &'static str,
    /// Where the rule stands.
    pub status: Status,
    /// What the specification does about it.
    pub stance: Stance,
    /// 1: a user meets it; 2: it governs an internal reading.
    pub level: u8,
    /// The date of the decision.
    pub since: &'static str,
    /// One line, English — a menu label.
    pub title: &'static str,
    /// What the rule does, English.
    pub text: &'static str,
    /// Why not otherwise, English.
    pub why: &'static str,
    /// Who decided it, when.
    pub rulings: &'static [Ruling],
    /// Where it is written down.
    pub refs: &'static [Ref],
    /// Where it shows.
    pub at: &'static [Anchor],
    /// The rules this one replaces.
    pub supersedes: &'static [&'static str],
    /// Former ids, never reused.
    pub aliases: &'static [&'static str],
    /// The releases it targets; empty is all of them.
    pub applies: &'static [&'static str],
    /// The setting that turns it off, if there is one.
    pub switch: Option<&'static str>,
}

/// A theme of the registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// The id rules name in `theme`.
    pub id: &'static str,
    /// English title.
    pub title: &'static str,
}

/// The texts of a rule or a theme in one language — and nothing else: a
/// translation has no right to change a reference, an anchor or a status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Translation {
    /// The language, as the directory under `rules/` names it.
    pub lang: &'static str,
    /// The rule's (or theme's) id.
    pub id: &'static str,
    /// The translated title, if translated.
    pub title: Option<&'static str>,
    /// The translated text, if translated.
    pub text: Option<&'static str>,
    /// The translated reason, if translated.
    pub why: Option<&'static str>,
}

include!(concat!(env!("OUT_DIR"), "/house.rs"));

/// Every rule, in file order (themes in `themes.toml` order, rules as written).
pub fn all() -> &'static [Rule] {
    RULES
}

/// The themes, in `themes.toml` order.
pub fn themes() -> &'static [Theme] {
    THEMES
}

/// A rule by id. An alias answers with the rule it became.
pub fn get(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.id == id || r.aliases.contains(&id))
}

/// The rules of one theme.
pub fn in_theme(theme: &str) -> impl Iterator<Item = &'static Rule> + '_ {
    RULES.iter().filter(move |r| r.theme == theme)
}

/// The rules anchored at a place — *which rules hold here?* This is the
/// reverse link the front end and the CLI call. The match is on the exact key.
pub fn at<'a>(kind: AnchorKind, key: &'a str) -> impl Iterator<Item = &'static Rule> + 'a {
    RULES
        .iter()
        .filter(move |r| r.at.iter().any(|a| a.kind == kind && a.key == key))
}

/// The rules that replace `id` — read a `superseded` rule beside what
/// supersedes it.
pub fn superseded_by(id: &str) -> impl Iterator<Item = &'static Rule> + '_ {
    RULES.iter().filter(move |r| r.supersedes.contains(&id))
}

/// The languages a translation exists in, English first.
pub fn locales() -> Vec<&'static str> {
    let mut found = vec!["en"];
    for t in TRANSLATIONS.iter().chain(THEME_TRANSLATIONS) {
        if !found.contains(&t.lang) {
            found.push(t.lang);
        }
    }
    found
}

/// One text and the language it is really in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text {
    /// The words.
    pub value: &'static str,
    /// `en` when it fell back, whatever was asked.
    pub lang: &'static str,
}

/// A rule's three texts in the language asked for, each in English where the
/// translation is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Localised {
    /// The title.
    pub title: Text,
    /// What the rule does.
    pub text: Text,
    /// Why.
    pub why: Text,
    /// True when a language other than English was asked for and at least one
    /// text is English: a missing word never passes for a language.
    pub fallback: bool,
}

fn pick(
    rule: &Rule,
    lang: &str,
    field: fn(&Translation) -> Option<&'static str>,
    english: &'static str,
) -> Text {
    if lang != "en"
        && let Some(t) = TRANSLATIONS.iter().find(|t| t.id == rule.id && t.lang == lang)
        && let Some(value) = field(t)
    {
        return Text { value, lang: t.lang };
    }
    Text {
        value: english,
        lang: "en",
    }
}

/// A rule's texts in `lang`, English where it is not translated, and the
/// answer says which.
pub fn text_in(rule: &Rule, lang: &str) -> Localised {
    let title = pick(rule, lang, |t| t.title, rule.title);
    let text = pick(rule, lang, |t| t.text, rule.text);
    let why = pick(rule, lang, |t| t.why, rule.why);
    let fallback = lang != "en" && [title, text, why].iter().any(|t| t.lang == "en");
    Localised {
        title,
        text,
        why,
        fallback,
    }
}

/// A theme's title in `lang`, English where it is not translated.
pub fn theme_title(theme: &Theme, lang: &str) -> Text {
    if lang != "en"
        && let Some(t) = THEME_TRANSLATIONS
            .iter()
            .find(|t| t.id == theme.id && t.lang == lang)
        && let Some(value) = t.title
    {
        return Text { value, lang: t.lang };
    }
    Text {
        value: theme.title,
        lang: "en",
    }
}
