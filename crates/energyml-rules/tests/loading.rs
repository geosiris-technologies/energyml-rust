//! The guards of the loader, tried on **fabricated** rule files: the shipped
//! registry cannot show that a guard fires, because it passed them all to
//! compile. Each test says what to break so that it reddens.

use crate::loader::{Error, Source, parse};

const THEMES: &str = "[[theme]]\nid = \"t\"\ntitle = \"T\"\n";

fn src(name: &str, text: impl Into<String>) -> Source {
    Source {
        name: name.into(),
        text: text.into(),
    }
}

/// A valid rule `house/t/<name>`, plus `extra` lines (which may override
/// nothing: TOML refuses a key twice, so a test that changes a field builds
/// its own text with `rule_with`).
fn rule(name: &str, status: &str, extra: &str) -> String {
    rule_with(&format!("house/t/{name}"), "t", status, "restates", extra)
}

fn rule_with(id: &str, theme: &str, status: &str, stance: &str, extra: &str) -> String {
    format!(
        "[[rule]]\nid = \"{id}\"\ntheme = \"{theme}\"\nstatus = \"{status}\"\nstance = \"{stance}\"\nlevel = 1\nsince = \"2026-10-02\"\ntitle = \"x\"\ntext = \"x\"\nwhy = \"x\"\n{extra}\n"
    )
}

fn load(rules: &[String], translations: &[(&str, String)]) -> Result<crate::loader::Parsed, Error> {
    let rules: Vec<Source> = rules
        .iter()
        .enumerate()
        .map(|(i, t)| src(&format!("rules/f{i}.toml"), t.clone()))
        .collect();
    let tr: Vec<(String, Source)> = translations
        .iter()
        .map(|(l, t)| (l.to_string(), src(&format!("{l}/x.toml"), t.clone())))
        .collect();
    parse(&src("rules/themes.toml", THEMES), &rules, &tr)
}

/// A superseded rule is accepted **only beside its replacement**, and the
/// history is readable from either end.
///
/// To break it: in `parse`, remove the `SupersededByNobody` or
/// `SupersedesNotSuperseded` check; the test goes red.
#[test]
fn a_superseded_rule_is_read_beside_its_replacement() {
    let old = rule("old", "superseded", "");
    let new = rule("new", "active", "supersedes = [\"house/t/old\"]");
    let parsed = load(&[old.clone(), new], &[]).expect("a rule and what replaces it load");
    let replacement = parsed
        .rules
        .iter()
        .find(|r| r.supersedes.iter().any(|s| s == "house/t/old"))
        .expect("the replacement names the old rule");
    assert_eq!(replacement.id, "house/t/new");
    assert_eq!(
        parsed
            .rules
            .iter()
            .find(|r| r.id == "house/t/old")
            .unwrap()
            .status,
        "superseded"
    );

    // Alone, a superseded rule is a history with no future: refused by name.
    assert_eq!(
        load(&[old], &[]).unwrap_err(),
        Error::SupersededByNobody {
            id: "house/t/old".into()
        }
    );
    // A replacement of a rule still in force would hide a live rule.
    let live = rule("live", "active", "");
    let new = rule("new", "active", "supersedes = [\"house/t/live\"]");
    assert_eq!(
        load(&[live, new], &[]).unwrap_err(),
        Error::SupersedesNotSuperseded {
            id: "house/t/new".into(),
            target: "house/t/live".into()
        }
    );
    // And one that replaces nothing that exists is a typo.
    let new = rule("new", "active", "supersedes = [\"house/t/ghost\"]");
    assert_eq!(
        load(&[new], &[]).unwrap_err(),
        Error::SupersedesUnknown {
            id: "house/t/new".into(),
            target: "house/t/ghost".into()
        }
    );
}

/// A translation that names an id which does not exist is refused **by its
/// name** — a French file that outlived a renamed rule must not rot quietly.
///
/// To break it: in `parse`, do not check `by_id.contains_key` before accepting
/// a translation; the test goes red.
#[test]
fn a_translation_naming_a_missing_id_is_refused_by_name() {
    let rules = [rule("a", "active", "")];
    let fr = "[[rule]]\nid = \"house/t/gone\"\ntitle = \"disparue\"\n".to_string();
    match load(&rules, &[("fr", fr)]) {
        Err(Error::UnknownTranslation { id, file }) => {
            assert_eq!(id, "house/t/gone");
            assert!(file.starts_with("fr/"), "the refusal says which file: {file}");
        }
        other => panic!("expected UnknownTranslation, got {other:?}"),
    }
    // The same file about an id that exists loads.
    let fr = "[[rule]]\nid = \"house/t/a\"\ntitle = \"une\"\n".to_string();
    let parsed = load(&rules, &[("fr", fr)]).unwrap();
    assert_eq!(parsed.translations.len(), 1);
}

/// A translation has no right to change a reference, an anchor or a status:
/// one that carries any other key is refused.
///
/// To break it: remove the key check in `translation`; the test goes red.
#[test]
fn a_translation_carries_only_id_title_text_why() {
    let rules = [rule("a", "active", "")];
    let fr = "[[rule]]\nid = \"house/t/a\"\ntitle = \"une\"\nstatus = \"superseded\"\n".to_string();
    assert!(matches!(load(&rules, &[("fr", fr)]), Err(Error::Field { field, .. }) if field == "status"));
    // Twice into the same language is two sources of the same fact.
    let fr =
        "[[rule]]\nid = \"house/t/a\"\ntitle = \"une\"\n\n[[rule]]\nid = \"house/t/a\"\ntitle = \"deux\"\n"
            .to_string();
    assert!(matches!(
        load(&rules, &[("fr", fr)]),
        Err(Error::DuplicateTranslation { .. })
    ));
}

/// Ids are unique across files, and an alias counts as an id.
///
/// To break it: remove the `seen` table from `parse`; the test goes red.
#[test]
fn ids_and_aliases_never_collide() {
    let a = rule("a", "active", "");
    assert!(matches!(
        load(&[a.clone(), a.clone()], &[]),
        Err(Error::DuplicateId { .. })
    ));
    let b = rule("b", "active", "aliases = [\"house/t/a\"]");
    assert!(matches!(load(&[a, b], &[]), Err(Error::DuplicateId { id, .. }) if id == "house/t/a"));
}

/// An id has one shape, `house/<theme>/<name>`; the theme field and the id
/// agree; only the two published ids are flat; the theme exists.
///
/// To break it: accept any flat id in `id_shape` (remove
/// `FLAT_IDS.contains`); the test goes red.
#[test]
fn the_shape_of_an_id_is_enforced() {
    let bad = |id: &str, theme: &str| load(&[rule_with(id, theme, "active", "restates", "")], &[]);
    assert!(bad("house/t/ok-name", "t").is_ok());
    assert!(
        matches!(bad("house/flat-one", "t"), Err(Error::Field { field, .. }) if field == "id"),
        "a new flat id is refused"
    );
    assert!(
        bad("house/discrete-value-in-lookup", "t").is_ok(),
        "the published flat ids are admitted"
    );
    assert!(
        matches!(bad("house/u/name", "t"), Err(Error::Field { field, .. }) if field == "theme"),
        "the id and the theme field disagree"
    );
    assert!(matches!(bad("house/t/Upper", "t"), Err(Error::Field { field, .. }) if field == "id"));
    assert!(matches!(bad("other/t/name", "t"), Err(Error::Field { field, .. }) if field == "id"));
    assert!(
        matches!(bad("house/z/name", "z"), Err(Error::UnknownTheme { .. })),
        "a theme `themes.toml` does not declare"
    );
}

/// Two ids that make the same Rust constant would make one shadow the other:
/// `house/a-b/c` and `house/a/b-c` both want `A_B_C`.
///
/// To break it: remove the `ConstantClash` check; the test goes red.
#[test]
fn two_ids_cannot_make_one_constant() {
    let themes = src(
        "rules/themes.toml",
        "[[theme]]\nid = \"a-b\"\ntitle = \"x\"\n\n[[theme]]\nid = \"a\"\ntitle = \"y\"\n",
    );
    let one = src(
        "rules/one.toml",
        rule_with("house/a-b/c", "a-b", "active", "restates", ""),
    );
    let two = src(
        "rules/two.toml",
        rule_with("house/a/b-c", "a", "active", "restates", ""),
    );
    match parse(&themes, &[one, two], &[]) {
        Err(Error::ConstantClash { constant, ids }) => {
            assert_eq!(constant, "A_B_C");
            assert_eq!(ids.len(), 2);
        }
        other => panic!("expected ConstantClash, got {other:?}"),
    }
    assert_eq!(
        crate::loader::constant_of("house/discrete-value-in-lookup"),
        "DISCRETE_VALUE_IN_LOOKUP"
    );
}

/// The English text is **obligatory**: a rule with no title, text or why, or an
/// empty one, does not load.
///
/// To break it: make `title` optional in `parse_rule`; the test goes red.
#[test]
fn the_english_is_present_everywhere() {
    for missing in ["title", "text", "why"] {
        let full = rule("a", "active", "");
        let without: String = full
            .lines()
            .filter(|l| !l.starts_with(&format!("{missing} =")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            matches!(load(&[without], &[]), Err(Error::Field { field, .. }) if field == missing),
            "{missing} may not be missing"
        );
        let empty = full.replace(&format!("{missing} = \"x\""), &format!("{missing} = \"  \""));
        assert!(
            matches!(load(&[empty], &[]), Err(Error::Field { field, .. }) if field == missing),
            "{missing} may not be empty"
        );
    }
}

/// A rule that says the specification is silent or ambiguous must say where it
/// looked: a `spec` reference, or a note. Otherwise "silent" is a claim with
/// no search behind it.
///
/// To break it: remove the `StanceWithoutLook` check; the test goes red.
#[test]
fn silent_must_say_where_it_looked() {
    let bare = rule_with("house/t/a", "t", "active", "silent", "");
    assert_eq!(
        load(&[bare], &[]).unwrap_err(),
        Error::StanceWithoutLook {
            id: "house/t/a".into()
        }
    );
    let noted = rule_with(
        "house/t/a",
        "t",
        "active",
        "silent",
        "refs = [ { kind = \"design\", path = \"docs/x.md\", note = \"no word in the TRG\" } ]",
    );
    assert!(load(&[noted], &[]).is_ok());
    let cited = rule_with(
        "house/t/a",
        "t",
        "active",
        "ambiguous",
        "refs = [ { kind = \"spec\", doc = \"common TRG 2.3\", at = \"§8.3 p. 188\" } ]",
    );
    assert!(load(&[cited], &[]).is_ok());
}

/// References are quoted and checked **by form**: the document of a `spec`
/// reference comes from a closed list, its place has a §, p. or l.; a path
/// stays under the root its kind lives in, and is never absolute.
///
/// To break it: remove the `one_of(.., SPEC_DOCS)` from `parse_rule`; the test goes red.
#[test]
fn references_are_checked_by_their_form() {
    let with = |r: &str| {
        load(
            &[rule_with(
                "house/t/a",
                "t",
                "active",
                "restates",
                &format!("refs = [ {r} ]"),
            )],
            &[],
        )
    };
    assert!(with("{ kind = \"spec\", doc = \"common TRG 2.3\", at = \"§8.3\" }").is_ok());
    assert!(
        with("{ kind = \"spec\", doc = \"Some Invented Guide\", at = \"§1\" }").is_err(),
        "a document outside the closed list"
    );
    assert!(
        with("{ kind = \"spec\", doc = \"common TRG 2.3\", at = \"somewhere\" }").is_err(),
        "a place with no section, page or line"
    );
    assert!(
        with("{ kind = \"spec\", doc = \"common TRG 2.3\" }").is_err(),
        "a spec reference with no place"
    );
    assert!(with("{ kind = \"design\", path = \"docs/adr/x.md\" }").is_ok());
    assert!(
        with("{ kind = \"design\", path = \"doc/design/x.md\" }").is_err(),
        "a document that does not ship with the repository"
    );
    assert!(
        with("{ kind = \"code\", path = \"web/src/x.ts\" }").is_err(),
        "code outside the crates"
    );
    assert!(
        with("{ kind = \"design\", path = \"/etc/passwd\" }").is_err(),
        "absolute"
    );
    assert!(
        with("{ kind = \"design\", path = \"crates/x.rs\" }").is_err(),
        "a design document lives under docs/"
    );
    assert!(
        with("{ kind = \"code\", path = \"crates/a/../../b.rs\" }").is_err(),
        "no parent segments"
    );
    assert!(
        with("{ kind = \"letter\", path = \"doc/x\" }").is_err(),
        "a kind outside the closed list"
    );
}

/// An anchor on a view names a view the registry declares; a typo is refused
/// (a rule that says it shows where nothing shows it is a bug of the registry).
///
/// To break it: remove the `VIEWS.contains` check; the test goes red.
#[test]
fn a_view_anchor_must_be_a_declared_view() {
    let with = |a: &str| {
        load(
            &[rule_with(
                "house/t/a",
                "t",
                "active",
                "restates",
                &format!("at = [ {a} ]"),
            )],
            &[],
        )
    };
    assert!(with("{ type = \"view\", key = \"object.colour-stack\" }").is_ok());
    assert!(with("{ type = \"view\", key = \"create.op:crs\" }").is_ok());
    assert!(with("{ type = \"view\", key = \"object.colour-stak\" }").is_err());
    assert!(with("{ type = \"type\", key = \"Attachment.Md\" }").is_ok());
    assert!(with("{ type = \"place\", key = \"x\" }").is_err());
}
