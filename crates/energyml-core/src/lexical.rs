//! The lexical grammar of the XSD built-ins this model keeps as strings.
//!
//! `xs:dateTime`, `xs:date`, `xs:time`, `xs:gYear`, `xs:gMonth`, `xs:gDay` and
//! `xs:duration` are generated as `String` (a reader that keeps them lexical
//! loses nothing on a round trip). The schema's own `pattern` on `TimeStamp`
//! (`.+T.+[Z+\-].*`) is a loose approximation: `2024-13-45T99:99:99Z` matches
//! it. A validator that reads the XSD (Xerces, the python tool) refuses it, so
//! this module says what the built-in itself says — XML Schema Part 2
//! (Datatypes) §3.3.7 *dateTime*, §3.3.9 *date*, §3.3.8 *time*, §3.3.11
//! *gYear*, §3.3.14 *gMonth*, §3.3.13 *gDay*, §3.3.6 *duration*, and the
//! timezone of §E.1 (`hh` up to 14, and then `mm` is 00).
//!
//! Year 0000 is refused (XSD 1.0, which the Energistics schemas target). Leap
//! seconds are refused: `ss` is 00 to 59, and `24:00:00` is the one accepted
//! hour 24.
//!
//! Surrounding whitespace is collapsed first, as `whiteSpace="collapse"` says.

/// Check `value` against the built-in named `kind`.
///
/// `Ok(())` when valid or when `kind` is not one this module knows (a type the
/// generator would not hand over); `Err(reason)` otherwise.
pub fn check(kind: &str, value: &str) -> Result<(), String> {
    let v = value.trim();
    let outcome = match kind {
        "dateTime" => date_time(v),
        "date" => date(v),
        "time" => time(v),
        "gYear" => g_year(v),
        "gMonth" => g_month(v),
        "gDay" => g_day(v),
        "duration" => duration(v),
        _ => return Ok(()),
    };
    outcome.map_err(|why| format!("`{value}` is not a valid xs:{kind}: {why}"))
}

type Res = Result<(), &'static str>;

/// Split a trailing timezone (`Z`, `+hh:mm`, `-hh:mm`) off, validating it.
fn split_timezone(s: &str) -> Result<&str, &'static str> {
    if let Some(rest) = s.strip_suffix('Z') {
        return Ok(rest);
    }
    let b = s.as_bytes();
    if b.len() >= 6 {
        let at = b.len() - 6;
        if (b[at] == b'+' || b[at] == b'-') && b[at + 3] == b':' {
            let (hh, mm) = (&s[at + 1..at + 3], &s[at + 4..]);
            let (Some(h), Some(m)) = (digits(hh), digits(mm)) else {
                return Err("malformed timezone");
            };
            if h > 14 || m > 59 || (h == 14 && m != 0) {
                return Err("timezone out of range (at most 14:00)");
            }
            return Ok(&s[..at]);
        }
    }
    Ok(s)
}

/// The value of an all-digit string of exactly the given length class.
fn digits(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn is_leap(year: u64) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u64, month: u64) -> u64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
    }
}

/// `-?YYYY…` : at least four digits, no leading zero beyond four, not 0000.
fn year(s: &str) -> Result<u64, &'static str> {
    let digits_part = s.strip_prefix('-').unwrap_or(s);
    if digits_part.len() < 4 || !digits_part.bytes().all(|c| c.is_ascii_digit()) {
        return Err("the year needs at least four digits");
    }
    if digits_part.len() > 4 && digits_part.starts_with('0') {
        return Err("a year of more than four digits has no leading zero");
    }
    let y: u64 = digits_part.parse().map_err(|_| "the year is too large")?;
    if y == 0 {
        return Err("there is no year 0000");
    }
    Ok(y)
}

/// `YYYY-MM-DD`, the date part (no timezone).
fn date_part(s: &str) -> Res {
    // The year may be negative, so the separators are found from the right.
    let b = s.as_bytes();
    if b.len() < 10 || b[b.len() - 3] != b'-' || b[b.len() - 6] != b'-' {
        return Err("expected YYYY-MM-DD");
    }
    let y = year(&s[..s.len() - 6])?;
    let m = digits(&s[s.len() - 5..s.len() - 3]).ok_or("the month is not two digits")?;
    let d = digits(&s[s.len() - 2..]).ok_or("the day is not two digits")?;
    if !(1..=12).contains(&m) {
        return Err("the month is not 01 to 12");
    }
    if d < 1 || d > days_in_month(y, m) {
        return Err("the day does not exist in that month");
    }
    Ok(())
}

/// `hh:mm:ss(.s+)?`, the time part (no timezone).
fn time_part(s: &str) -> Res {
    let (whole, fraction) = match s.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (s, None),
    };
    if let Some(f) = fraction
        && (f.is_empty() || !f.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err("malformed fraction of a second");
    }
    let parts: Vec<&str> = whole.split(':').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.len() != 2) {
        return Err("expected hh:mm:ss");
    }
    let (Some(h), Some(m), Some(sec)) = (digits(parts[0]), digits(parts[1]), digits(parts[2])) else {
        return Err("expected hh:mm:ss");
    };
    if h == 24 {
        let zero_fraction = fraction.is_none_or(|f| f.bytes().all(|c| c == b'0'));
        return if m == 0 && sec == 0 && zero_fraction {
            Ok(())
        } else {
            Err("hour 24 is only valid as 24:00:00")
        };
    }
    if h > 23 {
        return Err("the hour is not 00 to 23");
    }
    if m > 59 {
        return Err("the minute is not 00 to 59");
    }
    if sec > 59 {
        return Err("the second is not 00 to 59");
    }
    Ok(())
}

fn date_time(s: &str) -> Res {
    let body = split_timezone(s)?;
    let (d, t) = body
        .split_once('T')
        .ok_or("expected the `T` between date and time")?;
    date_part(d)?;
    time_part(t)
}

fn date(s: &str) -> Res {
    date_part(split_timezone(s)?)
}

fn time(s: &str) -> Res {
    time_part(split_timezone(s)?)
}

fn g_year(s: &str) -> Res {
    year(split_timezone(s)?).map(|_| ())
}

fn g_month(s: &str) -> Res {
    let body = split_timezone(s)?;
    // XSD 1.0's gMonth is `--MM--`, corrected to `--MM` in 1.0 second edition;
    // both are read.
    let body = body.strip_suffix("--").unwrap_or(body);
    let m = body
        .strip_prefix("--")
        .and_then(digits)
        .filter(|_| body.len() == 4);
    match m {
        Some(1..=12) => Ok(()),
        _ => Err("expected --MM with a month 01 to 12"),
    }
}

fn g_day(s: &str) -> Res {
    let body = split_timezone(s)?;
    let d = body
        .strip_prefix("---")
        .and_then(digits)
        .filter(|_| body.len() == 5);
    match d {
        Some(1..=31) => Ok(()),
        _ => Err("expected ---DD with a day 01 to 31"),
    }
}

/// `-?P(nY)?(nM)?(nD)?(T(nH)?(nM)?(n(.n)?S)?)?`, at least one component, and a
/// `T` is followed by at least one.
fn duration(s: &str) -> Res {
    let body = s.strip_prefix('-').unwrap_or(s);
    let Some(rest) = body.strip_prefix('P') else {
        return Err("a duration starts with P");
    };
    let (date_side, time_side) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let mut components = 0;
    let mut after = 0u8; // the designators already seen, to enforce order
    let mut num = String::new();
    for c in date_side.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let rank = match c {
            'Y' => 1,
            'M' => 2,
            'D' => 3,
            _ => return Err("unexpected character in the date part of the duration"),
        };
        if num.is_empty() || rank <= after {
            return Err("components are out of order or lack a number");
        }
        after = rank;
        num.clear();
        components += 1;
    }
    if !num.is_empty() {
        return Err("a number without a designator");
    }
    if let Some(t) = time_side {
        let mut after = 0u8;
        let mut seen = 0;
        let mut num = String::new();
        let mut fraction = false;
        for c in t.chars() {
            if c.is_ascii_digit() {
                num.push(c);
                continue;
            }
            if c == '.' && !fraction && !num.is_empty() {
                fraction = true;
                num.push(c);
                continue;
            }
            let rank = match c {
                'H' => 1,
                'M' => 2,
                'S' => 3,
                _ => return Err("unexpected character in the time part of the duration"),
            };
            if num.is_empty() || num.ends_with('.') || rank <= after || (fraction && rank != 3) {
                return Err("components are out of order or malformed");
            }
            after = rank;
            num.clear();
            fraction = false;
            seen += 1;
        }
        if !num.is_empty() {
            return Err("a number without a designator");
        }
        if seen == 0 {
            return Err("a `T` must be followed by at least one time component");
        }
        components += seen;
    }
    if components == 0 {
        return Err("a duration needs at least one component");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;

    fn ok(kind: &str, v: &str) {
        assert!(check(kind, v).is_ok(), "{kind} {v}: {:?}", check(kind, v));
    }
    fn bad(kind: &str, v: &str) {
        assert!(check(kind, v).is_err(), "{kind} {v} should be refused");
    }

    // XML Schema Part 2 §3.3.7 (dateTime) and its examples.
    #[test]
    fn date_time_follows_the_built_in() {
        ok("dateTime", "2026-10-04T12:30:00Z");
        ok("dateTime", "2026-10-04T12:30:00.123+02:00");
        ok("dateTime", "2026-10-04T12:30:00");
        ok("dateTime", "2026-10-04T24:00:00");
        ok("dateTime", "2024-02-29T00:00:00Z");
        ok("dateTime", "-0044-03-15T00:00:00Z");
        bad("dateTime", "2024-13-45T99:99:99Z");
        bad("dateTime", "2024-13-01T00:00:00Z");
        bad("dateTime", "2023-02-29T00:00:00Z");
        bad("dateTime", "2026-04-31T00:00:00Z");
        bad("dateTime", "2026-10-04T24:00:01Z");
        bad("dateTime", "2026-10-04T25:00:00Z");
        bad("dateTime", "2026-10-04T12:60:00Z");
        bad("dateTime", "2026-10-04T12:30:60Z");
        bad("dateTime", "2026-10-04 12:30:00Z");
        bad("dateTime", "2026-10-04T12:30:00+15:00");
        bad("dateTime", "2026-10-04T12:30:00+14:30");
        bad("dateTime", "0000-01-01T00:00:00Z");
        bad("dateTime", "26-10-04T00:00:00Z");
        bad("dateTime", "yesterday");
    }

    #[test]
    fn date_time_gregorian_and_gyear() {
        ok("date", "2026-10-04");
        ok("date", "2026-10-04Z");
        ok("date", "2026-10-04-05:00");
        bad("date", "2026-10-4");
        bad("date", "2026-02-30");
        bad("date", "2026-10-04T00:00:00Z");
        ok("time", "12:30:00");
        ok("time", "12:30:00.5Z");
        bad("time", "12:30");
        ok("gYear", "2026");
        ok("gYear", "12026");
        bad("gYear", "26");
        ok("gMonth", "--10");
        bad("gMonth", "--13");
        ok("gDay", "---31");
        bad("gDay", "---32");
    }

    #[test]
    fn duration_needs_a_component() {
        ok("duration", "P1Y2M3DT4H5M6.5S");
        ok("duration", "PT0S");
        ok("duration", "-P1D");
        bad("duration", "P");
        bad("duration", "PT");
        bad("duration", "P1D1Y");
        bad("duration", "1D");
        bad("duration", "P1.5D");
    }

    #[test]
    fn an_unknown_kind_is_not_judged() {
        ok("hexBinary", "anything");
    }
}
