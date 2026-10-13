//! An `xsd:dateTime` to Unix seconds, or a refusal that says why.
//!
//! `energyml_storage::epc::core_props::unix_seconds` reads the one form a
//! package's `core.xml` is written in — `Z`, whole seconds — and says `None`
//! for the rest. A series is read from files other people wrote: a PRODML
//! `dTim` carries an offset or a fraction as readily as a `Z`
//! (TimeSeriesData.xsd l. 100, the pattern `.+T.+[Z+\-].*` on `dTim`). This
//! reads the whole lexical form of XML Schema 1.0 Part 2 §3.2.7 —
//! `-?YYYY-MM-DDThh:mm:ss(.s+)?(Z|(+|-)hh:mm)?` — and refuses by name what it
//! cannot place:
//!
//! - **no time zone**: §3.2.7 calls such a value *"untimezoned"*; placing it on
//!   an axis of UTC seconds means choosing a zone for it, which is a guess;
//! - **a leap second** (`:60`): Unix seconds have no place for it;
//! - a day the month does not have, a field out of range.
//!
//! `24:00:00` is read as midnight of the next day, which §3.2.7 allows.

/// Seconds since 1970-01-01T00:00:00Z, fractions kept.
pub fn unix_seconds(text: &str) -> Result<f64, String> {
    let text = text.trim();
    let (date, rest) = text
        .split_once('T')
        .ok_or("no `T` between the date and the time")?;

    // The date: an optional minus, a year of four digits or more.
    let (negative, date) = match date.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, date),
    };
    let mut parts = date.splitn(3, '-');
    let year_text = parts.next().unwrap_or_default();
    let month_text = parts.next().ok_or("no month")?;
    let day_text = parts.next().ok_or("no day")?;
    if year_text.len() < 4 || !all_digits(year_text) || month_text.len() != 2 || day_text.len() != 2 {
        return Err("the date is not `YYYY-MM-DD`".into());
    }
    let year: i64 = year_text.parse().map_err(|_| "the year does not read")?;
    let year = if negative { -year } else { year };
    let month = two_digits(month_text).ok_or("the month does not read")?;
    let day = two_digits(day_text).ok_or("the day does not read")?;
    if !(1..=12).contains(&month) {
        return Err(format!("month {month} does not exist"));
    }
    if day < 1 || day > days_in_month(year, month) {
        return Err(format!("{year:04}-{month:02} has no day {day}"));
    }

    // The zone: `Z`, `+hh:mm`, `-hh:mm` — the last `+` or `-` after the time's
    // own digits, since the time holds none.
    let (time, offset_minutes) = if let Some(time) = rest.strip_suffix('Z') {
        (time, 0)
    } else if let Some(at) = rest.rfind(['+', '-']) {
        let (time, zone) = rest.split_at(at);
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let zone = &zone[1..];
        let (hours, minutes) = zone.split_once(':').ok_or("the offset is not `±hh:mm`")?;
        let hours = two_digits(hours).ok_or("the offset's hours do not read")?;
        let minutes = two_digits(minutes).ok_or("the offset's minutes do not read")?;
        if hours > 14 || minutes > 59 || (hours == 14 && minutes != 0) {
            return Err("the offset is outside ±14:00".into());
        }
        (time, sign * (hours * 60 + minutes))
    } else {
        return Err("no time zone: an untimezoned date-time has no place on an axis of UTC seconds without choosing a zone for it".into());
    };

    // The time: hh:mm:ss, a fraction optional.
    let mut fields = time.splitn(3, ':');
    let hour = two_digits(fields.next().unwrap_or_default()).ok_or("the hour does not read")?;
    let minute = two_digits(fields.next().ok_or("no minutes")?).ok_or("the minutes do not read")?;
    let seconds_text = fields.next().ok_or("no seconds")?;
    let (whole, fraction) = match seconds_text.split_once('.') {
        Some((whole, fraction)) => {
            if fraction.is_empty() || !all_digits(fraction) {
                return Err("the fraction of a second does not read".into());
            }
            (
                whole,
                format!("0.{fraction}")
                    .parse::<f64>()
                    .map_err(|_| "the fraction of a second does not read")?,
            )
        }
        None => (seconds_text, 0.0),
    };
    let second = two_digits(whole).ok_or("the seconds do not read")?;
    if second == 60 {
        return Err("a leap second: Unix seconds have no place for it".into());
    }
    if minute > 59 || second > 59 {
        return Err("a minute or a second out of range".into());
    }
    if hour > 24 || (hour == 24 && (minute != 0 || second != 0 || fraction != 0.0)) {
        return Err("an hour out of range".into());
    }

    let days = days_from_civil(year, month, day);
    let local = days * 86_400 + hour * 3600 + minute * 60 + second;
    Ok((local - offset_minutes * 60) as f64 + fraction)
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn two_digits(text: &str) -> Option<i64> {
    if text.len() == 2 && all_digits(text) {
        text.parse().ok()
    } else {
        None
    }
}

fn leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if leap(year) => 29,
        _ => 28,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date — Howard Hinnant's
/// `days_from_civil` (*chrono-Compatible Low-Level Date Algorithms*).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
