//! What a GeoSciML `Borehole` says, as SIGEOL exports write it.
//!
//! Reads the vocabulary and nothing else: no RESQML here, no store, no
//! geometry - the sibling of [`crate::sigeol`], which does the same for a
//! `GeologicUnit`. What it answers is *which borehole is this, where is its
//! head, how deep does it go, and at which depths does it change* - and where
//! the file does not say, it says so rather than guessing.
//!
//! # The measured depths are rebuilt from the intervals, and the zero is added
//!
//! A SIGEOL borehole carries no list of measured depths. It carries a list of
//! `logElement` - `BoreholeInterval`s, each with a `mappedIntervalBegin` and a
//! `mappedIntervalEnd` - and those bounds **are** the depths at which anything
//! about the hole is stated. So the frame's nodes are the *distinct* bounds,
//! sorted, and the trajectory's control points are the same.
//!
//! Two things that reading looks past, and both are named rather than hidden:
//!
//! - **the same depth range appears several times.** 22 117 of the 41 367
//!   intervals of the `Borehole_indent.json` test export are a repeat of a range
//!   another `logElement` already covers, because one to three geologic units
//!   are mapped onto one slice of hole. Depths are what this module reads, so
//!   the repeats collapse - [`Borehole::mds`] is a set, not a list, and
//!   [`Borehole::intervals`] keeps every one of them for whoever wants the
//!   geology;
//! - **the shallowest bound may not be zero.** Measured depths are taken from
//!   the ground, which is rarely listed: the zero is generally assumed to be the
//!   ground surface and does not appear in the list of depths. So a zero is
//!   prepended when the
//!   shallowest bound is deeper than the head, and [`Mds::zero_added`] says it
//!   was. In this very export the shallowest bound **is** zero on all 1 000
//!   boreholes, which is exactly why the case has a flag and a test rather
//!   than a comment: nothing in the corpus would exercise it.
//!
//! # The head is the geometry, the ground is the reference location
//!
//! The feature's own `geometry` is a 2D `Point` - longitude and latitude, no
//! third ordinate. The elevation lives beside it, in
//! `referenceLocation.properties.elevation`, on a nested `OriginPosition`
//! feature. On all 1 000 boreholes the two `Point`s hold the same pair, so the
//! head is read from the feature and the ground from the reference location,
//! and a disagreement between them would be a fact worth refusing on rather
//! than an average worth taking.
//!
//! # What the dates are, and what they are not
//!
//! `dateOfDrilling.endPosition` is `1965-11-20Z` - a **date**, in all 905 of
//! the boreholes that carry one. EML's `Creation` is an `xsd:dateTime`, so the
//! date is widened to midnight UTC and that widening is stated: it is a change
//! of precision, not a reading.

use serde_json::Value;

/// A quantity as the export writes it: a number and its unit code.
#[derive(Debug, Clone, PartialEq)]
pub struct Quantity {
    /// The number.
    pub value: f64,
    /// The unit, as the producer spells it - `m`.
    pub uom: String,
}

/// One `logElement`: a slice of hole, and what the producer says is in it.
///
/// The geologic unit is **an identifier and nothing else** - the export names
/// no lithology, no material, no measured property. That is why this crate
/// writes no property from it today, and why the reference is kept: the day a
/// SIGEOL export carries a description, what it hangs from is already read.
#[derive(Debug, Clone, PartialEq)]
pub struct Interval {
    /// The `BoreholeInterval`'s own id.
    pub id: Option<String>,
    /// The shallow end, in [`Interval::uom`].
    pub begin: f64,
    /// The deep end.
    pub end: f64,
    /// The unit both bounds are in.
    pub uom: String,
    /// `specification.@href` - the `GeologicUnit` this slice is said to be.
    pub unit: Option<String>,
    /// `collectionIdentifier.value` - `BSS000LKCG_GLU000AEKQ`, the producer's
    /// own key for *this unit in this hole*.
    pub collection: Option<String>,
}

/// The measured depths rebuilt from a borehole's intervals.
#[derive(Debug, Clone, PartialEq)]
pub struct Mds {
    /// Ascending, distinct. At least two, or the borehole is refused.
    pub values: Vec<f64>,
    /// The unit they are in.
    pub uom: String,
    /// Whether a zero was prepended because the shallowest bound was deeper
    /// than the head.
    pub zero_added: bool,
    /// The depth ranges no interval covers, between the shallowest bound and
    /// the deepest - the hole is described with holes in it, which is a fact
    /// about the file and not an error.
    pub gaps: Vec<(f64, f64)>,
}

/// What one `Borehole` feature says about itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Borehole {
    /// The feature's own `id` - `BSS000LKCG`.
    pub id: Option<String>,
    /// `parameter_id_forage`, the producer's permanent uuid for the hole.
    pub producer_id: Option<String>,
    /// `parameter_id_bss`, the national database code. Equal to [`Borehole::id`]
    /// on all 1 000 boreholes of the export, and read separately anyway: two
    /// fields that agree today are two fields.
    pub bss: Option<String>,
    /// `parameter_indice_bss` - `01536X0058`, the map-sheet index.
    pub indice: Option<String>,
    /// `identifier` - the `data.geoscience.fr` URL.
    pub identifier: Option<String>,
    /// `description` - `EPOS Borehole` on every one of them.
    pub description: Option<String>,
    /// `dateOfDrilling.endPosition`, **verbatim**: `1965-11-20Z`.
    pub drilled: Option<String>,
    /// What the hole was for, as the thesaurus titles it.
    pub purposes: Vec<String>,
    /// The URLs of those purposes, which is what a later pass would join on.
    pub purpose_hrefs: Vec<String>,
    /// `boreholeLength`.
    pub length: Option<Quantity>,
    /// `downholeDrillingDetails[0]`, the drilled interval.
    pub drilled_interval: Option<(Quantity, Quantity)>,
    /// The head, from the feature's own `Point`: longitude, latitude.
    pub head: Option<(f64, f64)>,
    /// `referenceLocation.properties.elevation` - the ground at the head.
    pub elevation: Option<f64>,
    /// The `logElement`s, every one of them, in the producer's order.
    pub intervals: Vec<Interval>,
}

impl Borehole {
    /// The title a written object gets: the BSS code, the feature's id, or the
    /// producer's uuid - in that order, and `None` when it has none of the
    /// three.
    pub fn title(&self) -> Option<String> {
        self.bss
            .clone()
            .or_else(|| self.id.clone())
            .or_else(|| self.producer_id.clone())
    }

    /// What names every object written for this hole.
    ///
    /// `parameter_id_forage` first, because it is a **permanent identifier the
    /// producer owns** - the same argument [`crate::naming`] makes for a
    /// lithostratigraphic unit's referential uuid. The BSS code is the
    /// fallback, and `None` means nothing here can be named deterministically.
    pub fn key(&self) -> Option<String> {
        self.producer_id
            .clone()
            .or_else(|| self.bss.clone())
            .or_else(|| self.id.clone())
    }

    /// The `parameter_id_forage`, when it really is a uuid.
    ///
    /// **Validated, never simply taken.** It is written straight into a
    /// `uuid` attribute, whose schema pins the shape; a producer that changed
    /// this field to a code tomorrow must give nothing rather than something
    /// that fails at write time, far from here.
    pub fn producer_uuid(&self) -> Option<&str> {
        self.producer_id
            .as_deref()
            .filter(|text| energyml_core::identifiers::is_uuid(text))
    }

    /// The measured depths, rebuilt from the intervals.
    ///
    /// `Err` names what is missing: a hole with no interval, one whose
    /// intervals disagree about their unit, one whose bounds are not finite,
    /// one that ends up with a single depth. None of those can be written as a
    /// frame, and each of them reads differently.
    pub fn mds(&self) -> Result<Mds, String> {
        if self.intervals.is_empty() {
            return Err("no `logElement`: nothing states a depth".to_string());
        }
        let uom = self.md_uom()?;
        let mut bounds: Vec<f64> = Vec::with_capacity(self.intervals.len() * 2);
        for interval in &self.intervals {
            if !interval.begin.is_finite() || !interval.end.is_finite() {
                return Err(format!(
                    "an interval bound is not a number: [{}, {}]",
                    interval.begin, interval.end
                ));
            }
            if interval.end < interval.begin {
                return Err(format!(
                    "an interval runs upwards: {} to {} - the file states a depth range backwards",
                    interval.begin, interval.end
                ));
            }
            bounds.push(interval.begin);
            bounds.push(interval.end);
        }
        bounds.sort_by(|a, b| a.partial_cmp(b).expect("finite, checked above"));
        bounds.dedup();

        // The gaps, measured on the distinct ranges rather than on the bounds:
        // two abutting intervals leave none, and a depth nothing covers is a
        // fact about the file.
        let mut ranges: Vec<(f64, f64)> = self.intervals.iter().map(|one| (one.begin, one.end)).collect();
        ranges.sort_by(|a, b| a.partial_cmp(b).expect("finite, checked above"));
        ranges.dedup();
        let mut gaps = Vec::new();
        let mut reached = f64::NEG_INFINITY;
        for (begin, end) in ranges {
            if reached.is_finite() && begin > reached {
                gaps.push((reached, begin));
            }
            reached = reached.max(end);
        }

        // The zero is the ground, and it is rarely listed.
        let zero_added = bounds[0] > 0.0;
        if zero_added {
            bounds.insert(0, 0.0);
        }
        if bounds.len() < 2 {
            return Err(format!(
                "the intervals state one depth only ({}) - a frame needs two",
                bounds[0]
            ));
        }
        Ok(Mds {
            values: bounds,
            uom,
            zero_added,
            gaps,
        })
    }

    /// The unit every interval is in, or why there is not exactly one.
    ///
    /// **Not defaulted to metres.** A borehole whose depths are in feet read as
    /// metres is a hole three times too short, and it opens cleanly.
    pub fn md_uom(&self) -> Result<String, String> {
        let mut seen: Vec<&str> = Vec::new();
        for interval in &self.intervals {
            if !seen.contains(&interval.uom.as_str()) {
                seen.push(&interval.uom);
            }
        }
        match seen.as_slice() {
            [] => Err("no `logElement`: no unit to read the depths in".to_string()),
            [""] => Err(
                "the intervals carry no `@uom`: the depths have no unit, and metres is not a safe guess"
                    .to_string(),
            ),
            [one] => Ok((*one).to_string()),
            many => Err(format!(
                "the intervals are in {} different units ({})",
                many.len(),
                many.join(", ")
            )),
        }
    }

    /// `dateOfDrilling` as an EML `Creation` - midnight UTC on that day.
    ///
    /// The export writes `1965-11-20Z`, a date; `Creation` is an
    /// `xsd:dateTime` whose pattern demands the `T`. Widening a date to
    /// midnight is a change of **precision**, so it is done in one place and
    /// said in the plan rather than scattered through the writer. Anything
    /// that already carries a `T` is passed through untouched, and anything
    /// this does not recognise gives `None` rather than a date with a guessed
    /// shape.
    pub fn drilled_at(&self) -> Option<String> {
        let raw = self.drilled.as_deref()?.trim();
        if raw.contains('T') {
            return Some(raw.to_string());
        }
        let day = raw.strip_suffix('Z').unwrap_or(raw);
        let bytes = day.as_bytes();
        let shaped = bytes.len() == 10
            && bytes.iter().enumerate().all(|(at, b)| match at {
                4 | 7 => *b == b'-',
                _ => b.is_ascii_digit(),
            });
        shaped.then(|| format!("{day}T00:00:00Z"))
    }
}

/// Whether a feature's `properties` say it is a borehole.
///
/// The `@featureType`, and not the presence of a `logElement`: a file that
/// calls something else a log would be read as a hole, and the producer's own
/// word for what a feature is is the one fact the format guarantees.
pub fn is_borehole(properties: &Value) -> bool {
    properties.get("@featureType").and_then(Value::as_str) == Some("Borehole")
}

/// Read a `Borehole` feature's `properties` and its own `Point`.
///
/// Returns `None` when the document says nothing this module recognises - not
/// an error: a collection may legitimately hold features of another kind, and
/// the caller decides whether that is a problem.
pub fn borehole_of(id: Option<&str>, head: Option<(f64, f64)>, properties: &Value) -> Option<Borehole> {
    if !is_borehole(properties) {
        return None;
    }
    let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_string);
    let details = properties
        .get("indexData")
        .and_then(|one| one.get("BoreholeDetails"));

    let mut parameters: Vec<(String, String)> = Vec::new();
    if let Some(list) = properties.get("parameter").and_then(Value::as_array) {
        for one in list {
            let title = one
                .get("name")
                .and_then(|n| n.get("@title"))
                .and_then(Value::as_str);
            let value = one.get("value").and_then(Value::as_str);
            if let (Some(title), Some(value)) = (title, value) {
                parameters.push((title.to_string(), value.to_string()));
            }
        }
    }
    let parameter = |wanted: &str| {
        parameters
            .iter()
            .find(|(title, _)| title == wanted)
            .map(|(_, value)| value.clone())
    };

    let mut purposes = Vec::new();
    let mut purpose_hrefs = Vec::new();
    if let Some(list) = details.and_then(|d| d.get("purpose")).and_then(Value::as_array) {
        for outer in list {
            let inner = outer
                .get("purpose")
                .and_then(Value::as_array)
                .map(|l| l.as_slice())
                .unwrap_or(&[]);
            for one in inner {
                if let Some(title) = one.get("@title").and_then(Value::as_str) {
                    purposes.push(title.to_string());
                }
                if let Some(href) = one.get("@href").and_then(Value::as_str) {
                    purpose_hrefs.push(href.to_string());
                }
            }
        }
    }

    let reference = properties
        .get("referenceLocation")
        .and_then(|one| one.get("properties"));
    let intervals = properties
        .get("logElement")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(interval_of).collect())
        .unwrap_or_default();

    Some(Borehole {
        id: id.map(str::to_string),
        producer_id: parameter("parameter_id_forage"),
        bss: parameter("parameter_id_bss"),
        indice: parameter("parameter_indice_bss"),
        identifier: text(properties.get("identifier")),
        description: text(properties.get("description")),
        drilled: text(
            details
                .and_then(|d| d.get("dateOfDrilling"))
                .and_then(|d| d.get("endPosition")),
        ),
        purposes,
        purpose_hrefs,
        length: quantity(details.and_then(|d| d.get("boreholeLength"))),
        drilled_interval: drilled_interval(properties),
        head,
        elevation: reference
            .and_then(|one| one.get("elevation"))
            .and_then(Value::as_f64),
        intervals,
    })
}

/// One `logElement`, when it is a `BoreholeInterval` with two bounds.
fn interval_of(entry: &Value) -> Option<Interval> {
    let properties = entry.get("properties")?;
    if properties.get("@featureType").and_then(Value::as_str) != Some("BoreholeInterval") {
        return None;
    }
    let begin = quantity(properties.get("mappedIntervalBegin"))?;
    let end = quantity(properties.get("mappedIntervalEnd"))?;
    // The two bounds of one interval in two units would be a range with no
    // length; the caller's `md_uom` sees only one of them, so it is settled here.
    let uom = if begin.uom == end.uom {
        begin.uom.clone()
    } else {
        String::new()
    };
    Some(Interval {
        id: entry.get("id").and_then(Value::as_str).map(str::to_string),
        begin: begin.value,
        end: end.value,
        uom,
        unit: properties
            .get("specification")
            .and_then(|one| one.get("@href"))
            .and_then(Value::as_str)
            .map(str::to_string),
        collection: properties
            .get("collectionIdentifier")
            .and_then(|one| one.get("value"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// The drilled interval, `downholeDrillingDetails[0]`.
fn drilled_interval(properties: &Value) -> Option<(Quantity, Quantity)> {
    let first = properties.get("downholeDrillingDetails")?.get(0)?;
    Some((
        quantity(first.get("intervalBegin"))?,
        quantity(first.get("intervalEnd"))?,
    ))
}

/// A `{ Quantity: { value, @uom } }`, wherever the export puts one.
fn quantity(value: Option<&Value>) -> Option<Quantity> {
    let inner = value?.get("Quantity")?;
    Some(Quantity {
        value: inner.get("value").and_then(Value::as_f64)?,
        uom: inner
            .get("@uom")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One borehole in the shape `Borehole_indent_extract_1_well.json` writes
    /// it - the real one, trimmed to two intervals.
    fn properties() -> Value {
        json!({
            "@featureType": "Borehole",
            "identifier": "https://data.geoscience.fr/id/Borehole/BSS000LKCG",
            "description": "EPOS Borehole",
            "indexData": { "BoreholeDetails": {
                "dateOfDrilling": { "endPosition": "1965-11-20Z" },
                "boreholeLength": { "Quantity": { "value": 15, "@uom": "m" } }
            }},
            "downholeDrillingDetails": [{
                "intervalBegin": { "Quantity": { "value": 0, "@uom": "m" } },
                "intervalEnd": { "Quantity": { "value": 15, "@uom": "m" } }
            }],
            "referenceLocation": {
                "type": "Feature",
                "geometry": { "type": "Point", "coordinates": [2.32, 48.9682] },
                "properties": { "@featureType": "OriginPosition", "elevation": 38.5 }
            },
            "logElement": [
                interval("6872447", 0.0, 0.5, "BSS000LKCG_38325d6d-f932-4164-82dc-c1fbfebf7563"),
                interval("6872444", 0.5, 6.4, "BSS000LKCG_29d030fc-45f2-4841-924d-7929992a2f16"),
                // The same slice again, as the export does 22 117 times: a
                // second unit mapped onto one range of hole.
                interval("6872445", 0.5, 6.4, "BSS000LKCG_GLU000AEKQ")
            ],
            "parameter": [
                { "name": { "@title": "parameter_id_forage" }, "value": "3ca79312-f3fa-4959-bae0-311c7a5c7359" },
                { "name": { "@title": "parameter_id_bss" }, "value": "BSS000LKCG" },
                { "name": { "@title": "parameter_indice_bss" }, "value": "01536X0058" }
            ]
        })
    }

    fn interval(id: &str, begin: f64, end: f64, collection: &str) -> Value {
        json!({
            "type": "Feature",
            "id": id,
            "geometry": { "type": "GeometryCollection", "geometries": [] },
            "properties": {
                "@featureType": "BoreholeInterval",
                "identifier": format!("https://data.geoscience.fr/id/BoreholeInterval/{id}"),
                "specification": { "@href": format!("https://data.geoscience.fr/id/GeologicUnit/{id}") },
                "mappedIntervalBegin": { "Quantity": { "value": begin, "@uom": "m" } },
                "mappedIntervalEnd": { "Quantity": { "value": end, "@uom": "m" } },
                "collectionIdentifier": { "value": collection, "@codeSpace": "BRGM" }
            }
        })
    }

    fn read() -> Borehole {
        borehole_of(Some("BSS000LKCG"), Some((2.32, 48.9682)), &properties()).expect("a borehole")
    }

    #[test]
    fn everything_the_export_states_about_a_hole_is_read() {
        let hole = read();
        assert_eq!(
            hole.producer_id.as_deref(),
            Some("3ca79312-f3fa-4959-bae0-311c7a5c7359")
        );
        assert_eq!(hole.bss.as_deref(), Some("BSS000LKCG"));
        assert_eq!(hole.indice.as_deref(), Some("01536X0058"));
        assert_eq!(hole.elevation, Some(38.5));
        assert_eq!(hole.head, Some((2.32, 48.9682)));
        assert_eq!(hole.length.as_ref().map(|q| q.value), Some(15.0));
        assert_eq!(hole.intervals.len(), 3);
        // The producer's uuid is what names the objects, and it is validated
        // rather than trusted.
        assert_eq!(hole.producer_uuid(), Some("3ca79312-f3fa-4959-bae0-311c7a5c7359"));
        assert_eq!(
            hole.key().as_deref(),
            Some("3ca79312-f3fa-4959-bae0-311c7a5c7359")
        );
        assert_eq!(hole.title().as_deref(), Some("BSS000LKCG"));
    }

    #[test]
    fn a_repeated_depth_range_is_one_depth_and_not_two() {
        // The 22 117 repeats: three intervals, four bounds, three of them
        // distinct. Counting them would put two frame nodes at the same MD,
        // and a zero-length interval between them.
        let mds = read().mds().expect("depths");
        assert_eq!(mds.values, vec![0.0, 0.5, 6.4]);
        assert_eq!(mds.uom, "m");
        assert!(!mds.zero_added, "this export's shallowest bound is already zero");
        assert!(mds.gaps.is_empty());
    }

    #[test]
    fn a_hole_whose_shallowest_bound_is_not_zero_gets_one_at_the_ground() {
        // The zero is the ground and it is rarely listed. No
        // borehole of the 1 000 exercises this, which is why it is a test:
        // the corpus would never make it fail.
        let mut props = properties();
        props["logElement"] = json!([interval("a", 3.0, 8.0, "k")]);
        let hole = borehole_of(Some("x"), Some((0.0, 0.0)), &props).expect("a borehole");
        let mds = hole.mds().expect("depths");
        assert_eq!(mds.values, vec![0.0, 3.0, 8.0]);
        assert!(mds.zero_added);
    }

    #[test]
    fn a_depth_nothing_covers_is_reported_and_not_smoothed_over() {
        // `BSS000NDQC` is the one hole of the export with gaps - three of them.
        let mut props = properties();
        props["logElement"] = json!([interval("a", 0.0, 2.0, "k"), interval("b", 2.5, 4.0, "k")]);
        let mds = borehole_of(Some("x"), Some((0.0, 0.0)), &props)
            .unwrap()
            .mds()
            .expect("depths");
        assert_eq!(mds.values, vec![0.0, 2.0, 2.5, 4.0]);
        assert_eq!(mds.gaps, vec![(2.0, 2.5)]);
    }

    #[test]
    fn the_unit_of_the_depths_is_read_and_never_defaulted() {
        // A hole in feet read as metres is a hole three times too short, and
        // it opens cleanly. So: refused by name.
        let mut props = properties();
        props["logElement"][0]["properties"]["mappedIntervalBegin"]["Quantity"]["@uom"] = json!("ft");
        props["logElement"][0]["properties"]["mappedIntervalEnd"]["Quantity"]["@uom"] = json!("ft");
        let why = borehole_of(Some("x"), None, &props)
            .unwrap()
            .md_uom()
            .expect_err("refused");
        assert!(why.contains("ft") && why.contains('m'), "{why}");

        let mut none = properties();
        none["logElement"] = json!([{
            "type": "Feature", "id": "a", "properties": {
                "@featureType": "BoreholeInterval",
                "mappedIntervalBegin": { "Quantity": { "value": 0 } },
                "mappedIntervalEnd": { "Quantity": { "value": 1 } }
            }
        }]);
        let why = borehole_of(Some("x"), None, &none)
            .unwrap()
            .md_uom()
            .expect_err("refused");
        assert!(why.contains("not a safe guess"), "{why}");
    }

    #[test]
    fn a_date_of_drilling_is_widened_to_midnight_and_said_to_be() {
        assert_eq!(read().drilled_at().as_deref(), Some("1965-11-20T00:00:00Z"));
        // Already a dateTime: untouched.
        let mut hole = read();
        hole.drilled = Some("1965-11-20T09:30:00Z".to_string());
        assert_eq!(hole.drilled_at().as_deref(), Some("1965-11-20T09:30:00Z"));
        // Not a shape this recognises: nothing, rather than a date invented
        // around a fragment.
        hole.drilled = Some("1965".to_string());
        assert_eq!(hole.drilled_at(), None);
        hole.drilled = None;
        assert_eq!(hole.drilled_at(), None);
    }

    #[test]
    fn a_hole_with_no_interval_states_no_depth_and_says_so() {
        let mut props = properties();
        props["logElement"] = json!([]);
        let why = borehole_of(Some("x"), None, &props)
            .unwrap()
            .mds()
            .expect_err("refused");
        assert!(why.contains("no `logElement`"), "{why}");
    }

    #[test]
    fn an_interval_stated_backwards_is_refused_rather_than_sorted() {
        // Sorting it would turn a producer's mistake into a depth range this
        // reader invented. None of the 41 367 intervals is backwards.
        let mut props = properties();
        props["logElement"] = json!([interval("a", 8.0, 3.0, "k")]);
        let why = borehole_of(Some("x"), None, &props)
            .unwrap()
            .mds()
            .expect_err("refused");
        assert!(why.contains("upwards"), "{why}");
    }

    #[test]
    fn a_feature_of_another_kind_is_not_a_borehole() {
        assert!(borehole_of(Some("x"), None, &json!({ "@featureType": "GeologicUnit" })).is_none());
        assert!(borehole_of(Some("x"), None, &Value::Null).is_none());
        assert!(!is_borehole(&json!({ "@featureType": "OriginPosition" })));
    }

    #[test]
    fn the_purposes_are_read_through_their_double_nesting() {
        let mut props = properties();
        props["indexData"]["BoreholeDetails"]["purpose"] = json!([{ "purpose": [
            { "@title": "EXPLOITATION/GEOTHERMIE/TRES-BASSE-ENERGIE", "@href": "https://data.geoscience.fr/ncl/FunMaMa/13" }
        ]}]);
        let hole = borehole_of(Some("x"), None, &props).expect("a borehole");
        assert_eq!(hole.purposes, vec!["EXPLOITATION/GEOTHERMIE/TRES-BASSE-ENERGIE"]);
        assert_eq!(hole.purpose_hrefs.len(), 1);
    }
}
