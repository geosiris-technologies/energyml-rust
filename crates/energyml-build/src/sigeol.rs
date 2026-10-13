//! What a GeoSciML `GeologicUnit` says: the GeoSciML GeologicUnit import
//! (tested with SIGEOL exports).
//!
//! Reads the vocabulary and nothing else: no RESQML here, no store, no
//! geometry. What it answers is *which unit is this feature about, between which
//! ages, and does it mark that unit's top or its base* - and where the file does
//! not say, it says so rather than guessing.
//!
//! # The inversion, measured and not assumed
//!
//! `geologicHistory[0]` carries `olderNamedAge` and `youngerNamedAge`, each with
//! a `@title` (a stage) and, under `numericAge.NumericAgeRange`, a bound in Ma.
//! In every one of the 16 402 features of the four SIGEOL exports tested,
//! **`olderBoundDate` holds the smaller number of Ma and `youngerBoundDate` the
//! larger**:
//!
//! | file | olderNamedAge | Ma | youngerNamedAge | Ma | n |
//! |---|---|---:|---|---:|---:|
//! | Priabonien | Priabonien | 33.9 | Bartonien | 41.2 | 7 |
//! | Bartonien | Bartonien | 37.8 | Lutétien | 47.8 | 10 354 |
//! | Bartonien | Bartonien | 37.8 | Bartonien | 41.2 | 20 |
//! | Lutétien | Lutétien | 41.2 | Lutétien | 47.8 | 5 776 |
//! | Yprésien | Yprésien | 47.8 | Yprésien | 56.0 | 245 |
//!
//! Fewer Ma is more recent, so the field *named* `older` holds the unit's
//! **young** end - its top - and the one named `younger` holds its base. The
//! names are inverted with respect to their content.
//!
//! **So the ends are ordered on the numbers, never on the field names.** Reading
//! the labels gives exactly the opposite answer, and both answers produce a file
//! that opens cleanly: a top set labelled as a bottom set is the failure this
//! module is written against. When there are no numeric bounds nothing is
//! claimed at all - the names are not evidence.
//!
//! # The side is decided against the **queried stage**, and one surface comes out
//!
//! The rule, and its shape, went through two wrong versions before
//! this one - both caught by the corpus, which is why it is a test and not a
//! reading exercise:
//!
//! - the first gave a unit one side chosen from its own two ends. It produced
//!   `Top` for two formations and nothing at all for the other five;
//! - the second gave every unit **both** surfaces. Symmetrical, and it invents
//!   an object: the export answers a question about *one* stage, and a unit's
//!   other end belongs to a different question.
//!
//! What decides is the stage the export was **asked about**. Against it:
//!
//! | the queried stage sits at | the points mark |
//! |---|---|
//! | the unit's young end (fewer Ma) | that stage's **Top** |
//! | the unit's old end (more Ma) | that stage's **Bottom** |
//! | neither | nothing - the unit does not touch it |
//!
//! A unit lying wholly inside one stage matches both ends; the young end is
//! tried first, so it yields a `Top`. That is a choice and it is written down
//! here rather than left to fall out of an `if`: the alternative would be to
//! emit two surfaces sharing one point cloud, which is the version this one
//! replaced.
//!
//! **And the queried stage is not in the file.** No SIGEOL export names it -
//! not in the collection header, not in a feature. It comes from the file name
//! (`GeologicUnit_<Stage>_pretty.json`, see [`stage_of_file_name`]) or from the
//! caller, and a caller that has neither gets no side at all rather than a
//! guessed one. That fragility is real and stated rather than hidden: it is the
//! price of the reading chosen, taken knowingly.

use serde_json::Value;

/// Which surface of a unit a feature marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    /// The unit's young end: its top.
    Top,
    /// The unit's old end: its base.
    Bottom,
}

impl Side {
    /// The word a title uses - the same the BRGM pipeline writes on its own
    /// boundary features (`Lutétien Top`, `Priabonien Bottom`).
    pub fn label(self) -> &'static str {
        match self {
            Side::Top => "Top",
            Side::Bottom => "Bottom",
        }
    }
}

/// One of the two surfaces a unit bounds, and the stage that surface lies in.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    /// Top or base of the unit.
    pub side: Side,
    /// The stage the surface lies in - the young end for a top, the old end
    /// for a base.
    pub stage: Age,
}

/// A named age with, when the producer wrote one, its bound in millions of years.
#[derive(Debug, Clone, PartialEq)]
pub struct Age {
    /// The stage: `Priabonien`, `Bartonien`.
    pub name: Option<String>,
    /// Its URI in the BRGM chronostratigraphic thesaurus
    /// (`https://data.geoscience.fr/ncl/GeolTs/1263`), when given. A permanent
    /// identifier is worth more than a label, and it is what a later pass over
    /// the thesaurus would join on.
    pub href: Option<String>,
    /// Millions of years before present. Larger is older.
    pub ma: Option<f64>,
}

impl Age {
    fn is_empty(&self) -> bool {
        self.name.is_none() && self.href.is_none() && self.ma.is_none()
    }
}

/// What one feature is about.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    /// The lithostratigraphic unit's name - « Marnes et Caillasses ».
    pub name: Option<String>,
    /// Its entry in the BRGM lithostratigraphic referential. The uuid at the
    /// end of this URL is a **permanent identifier the producer already owns**,
    /// which is why the objects built from it can be named deterministically
    /// rather than freshly each run.
    pub href: Option<String>,
    /// The young end of the unit - whatever the field carrying it is called.
    pub young: Age,
    /// The old end.
    pub old: Age,
    /// Whether the two ends could be **ordered**, which is what decides which is
    /// which. False when the producer wrote no numeric bounds, or equal ones.
    pub ordered: bool,
    /// Why the ends are ordered as they are, in one sentence - kept so a report
    /// can say it rather than leaving a reader to rediscover the inversion.
    pub because: &'static str,
}

impl Unit {
    /// The uuid of the referential entry, when the href carries one.
    ///
    /// Parsed off the end of the URL and **validated as a uuid**, never simply
    /// taken: `…/details/006fd552-1f63-47b3-bae9-e528f169083c` is the shape the
    /// BRGM writes today, and a referential that changed its URLs tomorrow must
    /// give nothing rather than a fragment that looks like an identifier.
    pub fn referential_uuid(&self) -> Option<&str> {
        let href = self.href.as_deref()?;
        let last = href.rsplit(['/', '#']).find(|part| !part.is_empty())?;
        energyml_core::identifiers::is_uuid(last).then_some(last)
    }

    /// Which surface of `stage` these points mark, and `None` when none.
    ///
    /// Three ways to get `None`, and they are three different facts a caller may
    /// want to tell apart - [`Unit::because`] says which:
    ///
    /// - the ends do not **order** (no numeric bounds, or equal ones), so young
    ///   and old are not established and nothing can be compared;
    /// - the queried stage matches **neither** end: the unit does not touch it,
    ///   which is ordinary in a file that answers about several units;
    /// - there is no queried stage to compare against, which the caller decides.
    ///
    /// The comparison ignores case and surrounding space, and nothing else. It
    /// does **not** fold accents: `Lutétien` and `Lutetien` are two spellings and
    /// matching them would mean deciding that a producer's `é` is noise. When a
    /// file and a thesaurus disagree on that, the fix is the stage passed in.
    pub fn side_for(&self, stage: &str) -> Option<Side> {
        if !self.ordered {
            return None;
        }
        let wanted = stage.trim();
        let same = |age: &Age| {
            age.name
                .as_deref()
                .is_some_and(|name| name.trim().eq_ignore_ascii_case(wanted))
        };
        // The young end first, and that order is the decision: a unit lying
        // wholly inside one stage matches both, and it comes out as that stage's
        // top rather than as two surfaces over one point cloud.
        if same(&self.young) {
            Some(Side::Top)
        } else if same(&self.old) {
            Some(Side::Bottom)
        } else {
            None
        }
    }

    /// The surface of `stage`, with the age that names it.
    pub fn surface_for(&self, stage: &str) -> Option<Surface> {
        let side = self.side_for(stage)?;
        let age = match side {
            Side::Top => self.young.clone(),
            Side::Bottom => self.old.clone(),
        };
        Some(Surface { side, stage: age })
    }

    /// A stable key for grouping: the unit, and which of its surfaces.
    ///
    /// The rule is to gather every point in the same point set when they are on
    /// the same unit and the same top/bottom - a group by exactly this.
    ///
    /// Keyed on the **referential uuid** when the file gives one: two producers
    /// spell a unit two ways, and a permanent identifier is what a referential
    /// exists to provide. The title is the fallback, and `?` the last resort -
    /// which groups every nameless unit together rather than losing them.
    pub fn key(&self, side: Side) -> (String, Side) {
        let name = self
            .referential_uuid()
            .map(str::to_string)
            .or_else(|| self.name.clone())
            .unwrap_or_else(|| "?".to_string());
        (name, side)
    }
}

/// The stage a SIGEOL export was asked about, read off its file name.
///
/// `GeologicUnit_Priabonien_pretty.json` → `Priabonien`. The suffixes the
/// exports carry are dropped, and **only those**: anything else between the
/// underscores is kept whole, because a stage name may contain them and this
/// reader has no list of stages to check against. The suffixes are the two
/// naming schemes of the producer -
///
/// - the first export: `_pretty`;
/// - the second (a later series of exports):
///   `GeologicUnit_<Stage>_<olderNamedAge|youngerNamedAge>_ZONE_<n>.json`, the
///   middle token naming the **field** the export was asked on (it is no part
///   of the stage), and `GeologicUnit_Priabonien_ZONE_2.json` carrying none.
///   Originally only `_pretty` was dropped, so `Bartonien_olderNamedAge_ZONE_2`
///   came out as a stage - which `sigeol-units` then refused, naming the
///   stages the file holds, instead of reading the right one.
///
/// **This is a convention, not a fact of the format**, and it is the weakest
/// link in the whole reading - no GeoJSON names the question it answers. It is
/// therefore only ever a *default*: a caller that knows the stage passes it, and
/// what this returns is what a person would have typed anyway. `None` when the
/// name does not follow the convention, never a guess at the first word.
pub fn stage_of_file_name(name: &str) -> Option<String> {
    let stem = name.rsplit(['/', '\\']).next()?;
    let stem = stem.strip_suffix(".json").unwrap_or(stem);
    let mut rest = stem.strip_prefix("GeologicUnit_")?.trim();
    // Peel the producer's own suffixes, in whatever order they come, until none
    // is left: none of them is part of a stage name.
    loop {
        let peeled = rest
            .strip_suffix("_pretty")
            .or_else(|| rest.strip_suffix("_olderNamedAge"))
            .or_else(|| rest.strip_suffix("_youngerNamedAge"))
            .or_else(|| zone_suffix(rest));
        match peeled {
            Some(shorter) => rest = shorter.trim(),
            None => break,
        }
    }
    (!rest.is_empty()).then(|| rest.to_string())
}

/// `name` without a trailing `_ZONE_<digits>`, when it has one.
fn zone_suffix(name: &str) -> Option<&str> {
    let (head, tail) = name.rsplit_once("_ZONE_")?;
    (!tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit())).then_some(head)
}

/// Read a feature's `properties`.
///
/// Returns `None` when the document says nothing this module recognises - not
/// an error: a collection may legitimately hold features of another kind, and
/// the caller decides whether that is a problem.
pub fn unit_of(properties: &Value) -> Option<Unit> {
    let litho = properties
        .get("composition")?
        .get(0)?
        .get("material")?
        .get("StratiRockMaterial")?
        .get("lithostratigraphy");
    let history = properties.get("geologicHistory").and_then(|h| h.get(0));

    let name = litho
        .and_then(|l| l.get("@title"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let href = litho
        .and_then(|l| l.get("@href"))
        .and_then(Value::as_str)
        .map(str::to_string);

    // The two ends, read under the names the producer uses and **kept apart from
    // what those names claim**: `older_field` is the field called `olderNamedAge`,
    // not the older age.
    let older_field = age(history, "olderNamedAge", "olderBoundDate");
    let younger_field = age(history, "youngerNamedAge", "youngerBoundDate");

    if name.is_none() && href.is_none() && older_field.is_empty() && younger_field.is_empty() {
        return None;
    }

    // Fewer Ma is more recent. Whichever field holds the smaller number is the
    // young end, whatever it is called.
    let (young, old, ordered, because) = match (older_field.ma, younger_field.ma) {
        (Some(a), Some(b)) if a < b => (
            older_field,
            younger_field,
            true,
            "the bounds decided it: the field named `older` holds the smaller Ma, so it is the young end",
        ),
        (Some(a), Some(b)) if b < a => (
            younger_field,
            older_field,
            true,
            "the bounds decided it: the field named `younger` holds the smaller Ma, so it is the young end",
        ),
        (Some(_), Some(_)) => (
            older_field,
            younger_field,
            false,
            "both bounds are the same age: there is no young end and no old one to tell apart",
        ),
        // **No bounds, no order.** Falling back on the field names is exactly the
        // reading that is inverted in this producer's files, and it would be
        // wrong silently. A caller that wants the surfaces must supply the ages.
        _ => (
            older_field,
            younger_field,
            false,
            "no numeric age: the field names alone are not evidence, they are inverted in the SIGEOL exports",
        ),
    };

    Some(Unit {
        name,
        href,
        young,
        old,
        ordered,
        because,
    })
}

fn age(history: Option<&Value>, named: &str, bound: &str) -> Age {
    let Some(history) = history else {
        return Age {
            name: None,
            href: None,
            ma: None,
        };
    };
    let named = history.get(named);
    Age {
        name: named
            .and_then(|n| n.get("@title"))
            .and_then(Value::as_str)
            .map(str::to_string),
        href: named
            .and_then(|n| n.get("@href"))
            .and_then(Value::as_str)
            .map(str::to_string),
        ma: history
            .get("numericAge")
            .and_then(|n| n.get("NumericAgeRange"))
            .and_then(|r| r.get(bound))
            .and_then(|b| b.get("Quantity"))
            .and_then(|q| q.get("value"))
            .and_then(Value::as_f64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One `GeologicUnit` in the shape the SIGEOL exports write it.
    fn properties(unit: &str, older: (&str, f64), younger: (&str, f64)) -> Value {
        json!({
            "@featureType": "GeologicUnit",
            "composition": [{
                "material": { "StratiRockMaterial": { "lithostratigraphy": {
                    "@href": "https://referentiel-lithostrat.brgm-rec.fr/#/lithostrat/search/details/006fd552-1f63-47b3-bae9-e528f169083c",
                    "@title": unit
                }}}
            }],
            "geologicHistory": [{
                "olderNamedAge": { "@title": older.0, "@href": "https://data.geoscience.fr/ncl/GeolTs/1263" },
                "youngerNamedAge": { "@title": younger.0, "@href": "https://data.geoscience.fr/ncl/GeolTs/1259" },
                "numericAge": { "NumericAgeRange": {
                    "olderBoundDate": { "Quantity": { "value": older.1, "uom": { "@code": "Ma" } } },
                    "youngerBoundDate": { "Quantity": { "value": younger.1, "uom": { "@code": "Ma" } } }
                }}
            }]
        })
    }

    #[test]
    fn the_young_end_is_the_smaller_number_of_ma_whatever_the_field_is_called() {
        // The real Priabonien feature: `olderNamedAge` = Priabonien at 33.9 Ma,
        // `youngerNamedAge` = Bartonien at 41.2. Fewer Ma is more recent, so the
        // field named `older` holds the young end - the unit's top.
        let unit = unit_of(&properties(
            "Formations indifférenciées du Calcaire de Saint-Ouen et du Calcaire de Champigny",
            ("Priabonien", 33.9),
            ("Bartonien", 41.2),
        ))
        .expect("a unit");
        assert_eq!(unit.young.name.as_deref(), Some("Priabonien"));
        assert_eq!(unit.old.name.as_deref(), Some("Bartonien"));

        // Asked about the Priabonien - which is the young end - the points mark
        // that stage's top. Asked about the Bartonien, the same points mark the
        // Bartonien's bottom: one export, one question, one surface.
        assert_eq!(unit.side_for("Priabonien"), Some(Side::Top));
        assert_eq!(unit.side_for("Bartonien"), Some(Side::Bottom));
        // A stage the unit does not touch gets nothing, which is not a failure.
        assert_eq!(unit.side_for("Yprésien"), None);
    }

    #[test]
    fn a_producer_writing_them_the_right_way_round_gets_the_same_answer() {
        // The counter-proof, and the reason the decision is on the numbers: a
        // file whose `olderNamedAge` really is the older age must still yield
        // the same young end. Reading the labels would flip here and only here.
        let unit = unit_of(&properties("X", ("Bartonien", 41.2), ("Priabonien", 33.9))).expect("a unit");
        assert_eq!(unit.young.name.as_deref(), Some("Priabonien"));
        assert_eq!(unit.old.name.as_deref(), Some("Bartonien"));
        assert_eq!(unit.side_for("Priabonien"), Some(Side::Top));
    }

    #[test]
    fn a_unit_inside_one_stage_comes_out_as_that_stages_top_and_only_that() {
        // Five of the seven SIGEOL formations, 16 128 features of 16 402. Both
        // ends match the queried stage; the young one is tried first, so one
        // surface comes out rather than two sharing a point cloud. Written down
        // in the module doc because it is a decision, not an accident of order.
        let unit = unit_of(&properties(
            "Calcaire grossier",
            ("Lutétien", 41.2),
            ("Lutétien", 47.8),
        ))
        .expect("a unit");
        assert_eq!(unit.side_for("Lutétien"), Some(Side::Top));
        assert_eq!(unit.side_for("Bartonien"), None);
    }

    #[test]
    fn the_comparison_ignores_case_and_space_and_nothing_else() {
        let unit = unit_of(&properties("X", ("Priabonien", 33.9), ("Bartonien", 41.2))).unwrap();
        assert_eq!(unit.side_for("  priabonien "), Some(Side::Top));
        // Accents are not folded: two spellings are two spellings, and deciding
        // that a producer's `é` is noise is not this module's call.
        let accented = unit_of(&properties("X", ("Lutétien", 41.2), ("Bartonien", 47.8))).unwrap();
        assert_eq!(accented.side_for("Lutetien"), None);
    }

    #[test]
    fn the_queried_stage_can_be_read_off_the_export_s_file_name() {
        assert_eq!(
            stage_of_file_name("GeologicUnit_Priabonien_pretty.json").as_deref(),
            Some("Priabonien")
        );
        assert_eq!(
            stage_of_file_name("C:/data/x/GeologicUnit_Lutétien_pretty.json").as_deref(),
            Some("Lutétien")
        );
        assert_eq!(
            stage_of_file_name("GeologicUnit_Yprésien.json").as_deref(),
            Some("Yprésien")
        );
        // Not the convention: nothing rather than the first word of a name that
        // means something else. This is a default, and a wrong default here
        // labels every surface in the package.
        // The second export's names: the field the export was
        // asked on and the zone are not part of the stage.
        assert_eq!(
            stage_of_file_name("GeologicUnit_Bartonien_olderNamedAge_ZONE_2.json").as_deref(),
            Some("Bartonien")
        );
        assert_eq!(
            stage_of_file_name("C:/data/x/GeologicUnit_Lutétien_youngerNamedAge_ZONE_2.json").as_deref(),
            Some("Lutétien")
        );
        assert_eq!(
            stage_of_file_name("GeologicUnit_Priabonien_ZONE_2.json").as_deref(),
            Some("Priabonien")
        );
        // Only what the producer writes is peeled: an underscore inside a name
        // stays, and `_ZONE_` without a number is not a zone.
        assert_eq!(
            stage_of_file_name("GeologicUnit_Haut_Lutétien_ZONE_2.json").as_deref(),
            Some("Haut_Lutétien")
        );
        assert_eq!(
            stage_of_file_name("GeologicUnit_A_ZONE_x.json").as_deref(),
            Some("A_ZONE_x")
        );
        assert_eq!(stage_of_file_name("GeologicUnit__ZONE_2.json"), None);
        assert_eq!(stage_of_file_name("horizons.geojson"), None);
        assert_eq!(stage_of_file_name("GeologicUnit_.json"), None);
    }

    #[test]
    fn without_numeric_ages_nothing_is_claimed_at_all() {
        // The field names are not evidence - they are inverted in exactly the
        // files this module was written for.
        let mut props = properties("X", ("Priabonien", 33.9), ("Bartonien", 41.2));
        props["geologicHistory"][0]["numericAge"] = Value::Null;
        let unit = unit_of(&props).expect("a unit");
        assert!(!unit.ordered);
        // Not even for a stage that plainly appears in the file: without an
        // order there is no young end to compare against.
        assert_eq!(unit.side_for("Priabonien"), None);
        assert!(unit.because.contains("inverted"), "{}", unit.because);
        // The stages are still read: what is refused is the *inference*, not the
        // data.
        assert_eq!(unit.young.name.as_deref(), Some("Priabonien"));
    }

    #[test]
    fn two_bounds_of_the_same_age_order_nothing() {
        let mut props = properties("X", ("A", 41.2), ("B", 41.2));
        props["geologicHistory"][0]["numericAge"]["NumericAgeRange"]["youngerBoundDate"]["Quantity"]["value"] =
            json!(41.2);
        let unit = unit_of(&props).expect("a unit");
        assert_eq!(unit.side_for("A"), None);
        assert!(unit.because.contains("same age"), "{}", unit.because);
    }

    #[test]
    fn the_referential_uuid_is_taken_from_the_url_and_validated() {
        let unit = unit_of(&properties("X", ("A", 1.0), ("B", 2.0))).expect("a unit");
        assert_eq!(
            unit.referential_uuid(),
            Some("006fd552-1f63-47b3-bae9-e528f169083c")
        );

        // A referential that changes its URLs gives nothing rather than a
        // fragment that merely looks like an identifier.
        let mut props = properties("X", ("A", 1.0), ("B", 2.0));
        props["composition"][0]["material"]["StratiRockMaterial"]["lithostratigraphy"]["@href"] =
            json!("https://example.org/lithostrat/marnes-et-caillasses");
        assert_eq!(unit_of(&props).expect("a unit").referential_uuid(), None);
    }

    #[test]
    fn the_group_key_is_the_unit_and_its_side() {
        let a = unit_of(&properties("X", ("Priabonien", 33.9), ("Bartonien", 41.2))).unwrap();
        let b = unit_of(&properties("X", ("Priabonien", 33.9), ("Bartonien", 41.2))).unwrap();
        assert_eq!(a.key(Side::Top), b.key(Side::Top));
        // The uuid and not the title: two producers spell a unit two ways, and a
        // permanent identifier is what the referential exists to give.
        assert_eq!(a.key(Side::Top).0, "006fd552-1f63-47b3-bae9-e528f169083c");
    }

    #[test]
    fn a_feature_of_another_kind_is_not_a_unit_and_says_so_by_being_none() {
        assert!(unit_of(&json!({ "title": "a polygon somebody drew" })).is_none());
        assert!(unit_of(&Value::Null).is_none());
    }
}
