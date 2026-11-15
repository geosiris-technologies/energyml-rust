//! The proleptic Gregorian calendar, as day counts: the one copy of the civil
//! date arithmetic of the workspace.
//!
//! Howard Hinnant's `days_from_civil` and `civil_from_days` (*chrono-Compatible
//! Low-Level Date Algorithms*), exact for every date this software will see.
//! The W3CDTF reader and writer of `core.xml` (`energyml-storage`), the
//! instant reader of the series (`energyml-domain`) and the clock of the CLI
//! all go through these two functions; none keeps its own.

/// A civil date to days since 1970-01-01 (negative before it).
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Days since 1970-01-01 to a civil date `(year, month, day)`: the inverse of
/// [`days_from_civil`].
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, days_from_civil};

    /// Dates checked against the calendar itself, not against this arithmetic:
    /// the Unix epoch, a leap day, the century that is not a leap year, a date
    /// before 1970 (the sign and era slips).
    ///
    /// Break it: use `/` instead of `div_euclid` in `days_from_civil`.
    #[test]
    fn known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
        assert_eq!(
            days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28),
            2,
            "2000 is a leap year"
        );
        assert_eq!(
            days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28),
            1,
            "1900 is not"
        );
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    /// The two functions are inverses over a span of centuries.
    ///
    /// Break it: change a constant of `civil_from_days`.
    #[test]
    fn the_two_are_inverses() {
        for days in (-100_000..100_000).step_by(37) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, i64::from(m), i64::from(d)), days);
        }
    }
}
