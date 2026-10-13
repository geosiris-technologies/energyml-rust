// The BRGM chronostratigraphic chart, as `data.geoscience.fr` exports it.
//
// **Included verbatim by `build.rs` and by `geolts.rs`**, the pattern of
// `dictionary_xml.rs`: the build-time compaction and a run-time reading of a
// fresh export can never disagree on what a column means. So this file uses
// nothing outside `std`, and nothing here is `pub` at the crate's surface —
// `geolts` wraps it.
//
// The export is a SKOS register flattened into CSV (RFC 4180): one concept
// per line, one column per predicate, several values in one cell separated by
// `|`, literals written `'text'@lang` with `\'` for a quote, URIs in `<…>`.
// Measured on the snapshot of 24/09/2026: 698 concepts, 497 bounds.

/// One concept, reduced to what a stratigraphic column needs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawConcept {
    /// The number that ends its URI: `…/GeolTs/1263` is 1263.
    pub(crate) id: u32,
    /// The French label — the register's reference language.
    pub(crate) label_fr: String,
    /// The English label, when there is one.
    pub(crate) label_en: String,
    /// `skos:notation`: the map code, `e7` for the Priabonien. Several are
    /// possible and **none is unique**: `e7` names four concepts.
    pub(crate) notations: Vec<String>,
    /// The BRGM mnemonic, `PRIA`.
    pub(crate) mnemonic: String,
    /// `DivChronoStrat/n`: 1 supereonothem … 7 stage, 8 substage.
    pub(crate) level: u8,
    /// `natelem/n`: 1 reference, 2 regional, 3 usual, 4 obsolete, 5 informal.
    pub(crate) nature: u8,
    /// `skos:broader`, every one of them — a concept may name its series
    /// **and** its sub-series.
    pub(crate) parents: Vec<u32>,
    /// The chart's colour, `[r, g, b]`, or none.
    pub(crate) colour: Option<[u8; 3]>,
    /// Where it begins, in millions of years before present — the **old** end.
    pub(crate) begins_ma: Option<f64>,
    /// Where it ends — the **young** end.
    pub(crate) ends_ma: Option<f64>,
    /// `valid`, `retired`, `submitted` — INSPIRE's status.
    pub(crate) status: String,
    /// `dct:spatial`: `Monde`, `Europe`, `Bassin de Paris`…
    pub(crate) spatial: String,
    /// `dct:source`: the reference the division is taken from — `Gradstein et
    /// al. (2012)`. Filled for 593 of 698 on the snapshot.
    pub(crate) source: String,
    /// The register's note on the division (`skos:skopeNote`, the export's own
    /// spelling) — 154 of 698.
    pub(crate) note: String,
    /// `skos:altLabel`, its other names — 43 of 698.
    pub(crate) alt_labels: Vec<String>,
    /// Whether its base is defined by a GSSP, or a GSSA.
    pub(crate) gssp: bool,
    pub(crate) gssa: bool,
}

/// The records of a CSV text: RFC 4180, a quoted field may hold the
/// separator, a doubled quote and a line break.
pub(crate) fn csv_records(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut line = 1usize;
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                line += 1;
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(format!("a quoted field opened before line {line} is never closed"));
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

/// The values of a cell: `a|b|c`, **outside** quotes — a label may hold a `|`.
fn values(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_literal = false;
    let mut chars = cell.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_literal => {
                if let Some(next) = chars.next() {
                    current.push('\\');
                    current.push(next);
                }
            }
            '\'' => {
                in_literal = !in_literal;
                current.push(c);
            }
            '|' if !in_literal => out.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.into_iter().map(|one| one.trim().to_string()).filter(|one| !one.is_empty()).collect()
}

/// `'Priabonien'@fr` → (`Priabonien`, `fr`); `'253'` → (`253`, ``). A bare
/// value is returned as it stands.
fn literal(value: &str) -> (String, String) {
    let Some(rest) = value.strip_prefix('\'') else { return (value.to_string(), String::new()) };
    let Some(end) = rest.rfind('\'') else { return (rest.to_string(), String::new()) };
    let text = rest[..end].replace("\\'", "'").replace("\\\\", "\\");
    let lang = rest[end + 1..].strip_prefix('@').unwrap_or("").to_string();
    (text, lang)
}

/// The number that ends a URI: `<https://…/GeolTs/1263>` → 1263.
fn tail(uri: &str) -> Option<u32> {
    uri.trim().trim_start_matches('<').trim_end_matches('>').rsplit('/').next()?.parse().ok()
}

/// The column of a header, whatever the prefix it is written with.
fn column(headers: &[String], suffix: &str) -> Option<usize> {
    headers.iter().position(|one| {
        let one = one.trim().trim_start_matches('<').trim_end_matches('>');
        one == suffix || one.ends_with(&format!("#{suffix}")) || one.ends_with(&format!("/{suffix}")) || one.ends_with(&format!(":{suffix}"))
    })
}

/// The ages of the bounds, by id: `GeolTsBound/1263` → 33.9.
pub(crate) fn bounds(text: &str) -> Result<Vec<(u32, f64)>, String> {
    let records = csv_records(text)?;
    let Some((headers, rows)) = records.split_first() else { return Ok(Vec::new()) };
    let id = column(headers, "@id").ok_or("the bounds export has no `@id` column")?;
    let at = column(headers, "numericPosition").ok_or("the bounds export has no `time:numericPosition` column")?;
    let mut out = Vec::new();
    for row in rows {
        let (Some(id), Some(position)) = (row.get(id).and_then(|one| tail(one)), row.get(at)) else { continue };
        let Some(first) = values(position).into_iter().next() else { continue };
        // **A decimal comma**: the export writes `41,2` for 41.2 Ma — measured
        // on the snapshot, next to `'56'` for a whole number. Parsed as a point
        // number only, every fractional age would be dropped without a word,
        // and the stages would lose their order.
        let text = literal(&first).0.trim().replace(',', ".");
        match text.parse::<f64>() {
            Ok(ma) => out.push((id, ma)),
            Err(_) => return Err(format!("the bound {id} states `{text}` as its age, which is not a number")),
        }
    }
    Ok(out)
}

/// The concepts of a GeolTs export, with their ages when the bounds are given.
pub(crate) fn concepts(text: &str, ages: &[(u32, f64)]) -> Result<Vec<RawConcept>, String> {
    let records = csv_records(text)?;
    let Some((headers, rows)) = records.split_first() else {
        return Err("the export is empty".to_string());
    };
    let need = |name: &str| column(headers, name).ok_or_else(|| format!("the export has no `{name}` column"));
    let id_at = need("@id")?;
    let label_at = need("prefLabel")?;
    let optional = |name: &str| column(headers, name);
    let (notation_at, mnemo_at, level_at, nature_at, broader_at) = (
        optional("notation"),
        optional("mnemo"),
        optional("typeChronostratAge"),
        optional("nature"),
        optional("broader"),
    );
    let (red_at, green_at, blue_at) =
        (optional("rgbCoordinateRed"), optional("rgbCoordinateGreen"), optional("rgbCoordinateBlue"));
    let (begin_at, end_at, status_at, spatial_at) =
        (optional("hasBeginning"), optional("hasEnd"), optional("status"), optional("spatial"));
    // `skopeNote` is how the export spells it; `scopeNote` is SKOS's own.
    let (source_at, note_at, alt_at, gssp_at, gssa_at) = (
        optional("source"),
        optional("skopeNote").or_else(|| optional("scopeNote")),
        optional("altLabel"),
        optional("isGSSP"),
        optional("isGSSA"),
    );
    let age = |id: Option<u32>| id.and_then(|id| ages.iter().find(|(one, _)| *one == id).map(|(_, ma)| *ma));
    let cell = |row: &Vec<String>, at: Option<usize>| -> String { at.and_then(|at| row.get(at)).cloned().unwrap_or_default() };

    let mut out = Vec::new();
    for row in rows {
        let Some(id) = row.get(id_at).and_then(|one| tail(one)) else { continue };
        let mut label_fr = String::new();
        let mut label_en = String::new();
        for value in values(&cell(row, Some(label_at))) {
            match literal(&value) {
                (text, lang) if lang == "fr" => label_fr = text,
                (text, lang) if lang == "en" => label_en = text,
                (text, _) if label_fr.is_empty() => label_fr = text,
                _ => {}
            }
        }
        if label_fr.is_empty() {
            label_fr = label_en.clone();
        }
        let number = |at: Option<usize>| -> Option<u8> {
            values(&cell(row, at)).into_iter().next().and_then(|one| literal(&one).0.trim().parse().ok())
        };
        let colour = match (number(red_at), number(green_at), number(blue_at)) {
            (Some(r), Some(g), Some(b)) => Some([r, g, b]),
            _ => None,
        };
        out.push(RawConcept {
            id,
            label_fr,
            label_en,
            notations: values(&cell(row, notation_at)).into_iter().map(|one| literal(&one).0).collect(),
            mnemonic: values(&cell(row, mnemo_at)).into_iter().next().map(|one| literal(&one).0).unwrap_or_default(),
            level: values(&cell(row, level_at)).first().and_then(|one| tail(one)).unwrap_or(0) as u8,
            nature: values(&cell(row, nature_at)).first().and_then(|one| tail(one)).unwrap_or(0) as u8,
            parents: values(&cell(row, broader_at)).iter().filter_map(|one| tail(one)).collect(),
            colour,
            begins_ma: age(values(&cell(row, begin_at)).first().and_then(|one| tail(one))),
            ends_ma: age(values(&cell(row, end_at)).first().and_then(|one| tail(one))),
            status: values(&cell(row, status_at))
                .first()
                .map(|one| one.trim_end_matches('>').rsplit('/').next().unwrap_or("").to_string())
                .unwrap_or_default(),
            spatial: values(&cell(row, spatial_at)).into_iter().next().map(|one| literal(&one).0).unwrap_or_default(),
            source: values(&cell(row, source_at)).into_iter().map(|one| literal(&one).0).collect::<Vec<_>>().join("; "),
            note: values(&cell(row, note_at)).into_iter().map(|one| literal(&one).0).collect::<Vec<_>>().join(" "),
            alt_labels: values(&cell(row, alt_at)).into_iter().map(|one| literal(&one).0).filter(|one| !one.is_empty()).collect(),
            gssp: values(&cell(row, gssp_at)).first().is_some_and(|one| literal(one).0 == "true"),
            gssa: values(&cell(row, gssa_at)).first().is_some_and(|one| literal(one).0 == "true"),
        });
    }
    if out.is_empty() {
        return Err("no concept in the export carries an `@id`".to_string());
    }
    Ok(out)
}

/// One concept per line, tab-separated, lists joined by `|`.
pub(crate) fn to_compact(concepts: &[RawConcept]) -> String {
    let clean = |text: &str| text.replace(['\t', '\n', '\r'], " ");
    let number = |value: Option<f64>| value.map(|one| format!("{one:?}")).unwrap_or_default();
    let mut out = String::new();
    for one in concepts {
        let fields = [
            one.id.to_string(),
            clean(&one.label_fr),
            clean(&one.label_en),
            one.notations.iter().map(|n| clean(n).replace('|', "/")).collect::<Vec<_>>().join("|"),
            clean(&one.mnemonic),
            one.level.to_string(),
            one.nature.to_string(),
            one.parents.iter().map(u32::to_string).collect::<Vec<_>>().join("|"),
            one.colour.map(|[r, g, b]| format!("{r},{g},{b}")).unwrap_or_default(),
            number(one.begins_ma),
            number(one.ends_ma),
            clean(&one.status),
            clean(&one.spatial),
            clean(&one.source),
            clean(&one.note),
            one.alt_labels.iter().map(|n| clean(n).replace('|', "/")).collect::<Vec<_>>().join("|"),
            if one.gssp { "1".to_string() } else { "0".to_string() },
            if one.gssa { "1".to_string() } else { "0".to_string() },
        ];
        out.push_str(&fields.join("\t"));
        out.push('\n');
    }
    out
}

/// The reverse of [`to_compact`].
pub(crate) fn from_compact(text: &str) -> Vec<RawConcept> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 13 {
                return None;
            }
            let list = |text: &str| -> Vec<String> {
                text.split('|').filter(|one| !one.is_empty()).map(str::to_string).collect()
            };
            let colour = {
                let parts: Vec<u8> = f[8].split(',').filter_map(|one| one.parse().ok()).collect();
                (parts.len() == 3).then(|| [parts[0], parts[1], parts[2]])
            };
            Some(RawConcept {
                id: f[0].parse().ok()?,
                label_fr: f[1].to_string(),
                label_en: f[2].to_string(),
                notations: list(f[3]),
                mnemonic: f[4].to_string(),
                level: f[5].parse().unwrap_or(0),
                nature: f[6].parse().unwrap_or(0),
                parents: f[7].split('|').filter_map(|one| one.parse().ok()).collect(),
                colour,
                begins_ma: f[9].parse().ok(),
                ends_ma: f[10].parse().ok(),
                status: f[11].to_string(),
                spatial: f[12].to_string(),
                // Later fields, absent from an older compaction: empty, never
                // a refusal.
                source: f.get(13).map(|one| one.to_string()).unwrap_or_default(),
                note: f.get(14).map(|one| one.to_string()).unwrap_or_default(),
                alt_labels: f.get(15).map(|one| list(one)).unwrap_or_default(),
                gssp: f.get(16).is_some_and(|one| *one == "1"),
                gssa: f.get(17).is_some_and(|one| *one == "1"),
            })
        })
        .collect()
}
