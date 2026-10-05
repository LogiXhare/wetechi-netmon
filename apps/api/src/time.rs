//! RFC 3339 UTC timestamps for the API, from the microseconds the
//! database and the domain store.
//!
//! Hand-written, on Howard Hinnant's `civil_from_days`, rather than adding
//! a date crate's formatting features for one function. Whole seconds are
//! written without a fraction (`2026-08-22T14:03:11Z`), and anything finer
//! with exactly six digits.

/// `micros` since the Unix epoch, as RFC 3339 UTC.
pub fn rfc3339(micros: i64) -> String {
    let seconds = micros.div_euclid(1_000_000);
    let fraction = micros.rem_euclid(1_000_000);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let of_day = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (of_day / 3_600, of_day % 3_600 / 60, of_day % 60);
    if fraction == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{fraction:06}Z")
    }
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.f]Z` (UTC only, up to six fractional
/// digits; `+00:00` is accepted for `Z`) to microseconds since the epoch.
/// Anything else, including an impossible date, is `None`.
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let text = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix("+00:00"))?;
    let (date, time) = text.split_once('T')?;
    let number = |part: &str, len: usize| -> Option<i64> {
        (part.len() == len && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    let mut date_parts = date.split('-');
    let year = number(date_parts.next()?, 4)?;
    let month = number(date_parts.next()?, 2)?;
    let day = number(date_parts.next()?, 2)?;
    if date_parts.next().is_some() {
        return None;
    }
    let (clock, fraction) = match time.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (time, None),
    };
    let mut clock_parts = clock.split(':');
    let hour = number(clock_parts.next()?, 2)?;
    let minute = number(clock_parts.next()?, 2)?;
    let second = number(clock_parts.next()?, 2)?;
    if clock_parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let micros = match fraction {
        None => 0,
        Some(digits) if (1..=6).contains(&digits.len()) => {
            number(digits, digits.len())? * 10_i64.pow(6 - digits.len() as u32)
        }
        Some(_) => return None,
    };
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(((days * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000_000) + micros)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// (year, month, day) to days since 1970-01-01, the inverse of
/// [`civil_from_days`].
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn known_instants() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_700_000_000_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(
            rfc3339(1_700_000_000_000_001),
            "2023-11-14T22:13:20.000001Z"
        );
        // A leap day, and the end of a leap year.
        assert_eq!(rfc3339(951_782_400_000_000), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_735_603_199_000_000), "2024-12-30T23:59:59Z");
        // Before the epoch.
        assert_eq!(rfc3339(-1), "1969-12-31T23:59:59.999999Z");
    }

    #[test]
    fn parsing_accepts_utc_and_refuses_the_rest() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339("2023-11-14T22:13:20.5Z"),
            Some(1_700_000_000_500_000)
        );
        assert_eq!(
            parse_rfc3339("2023-11-14T22:13:20+00:00"),
            Some(1_700_000_000_000_000)
        );
        for text in [
            "2023-11-14",
            "2023-11-14T22:13:20",
            "2023-11-14T22:13:20+06:00",
            "2023-02-29T00:00:00Z",
            "2023-13-01T00:00:00Z",
            "2023-11-14T24:00:00Z",
            "2023-11-14T22:13:20.1234567Z",
            "23-11-14T22:13:20Z",
            "2023-11-14T22:13:2xZ",
        ] {
            assert_eq!(parse_rfc3339(text), None, "{text}");
        }
        assert!(parse_rfc3339("2024-02-29T00:00:00Z").is_some());
    }

    proptest! {
        /// What is written can be read back exactly.
        #[test]
        fn parse_inverts_render(micros in -10_000_000_000_000_000i64..10_000_000_000_000_000) {
            prop_assert_eq!(parse_rfc3339(&rfc3339(micros)), Some(micros));
        }

        /// Agrees with the standard library's own day arithmetic: the
        /// rendered date, re-parsed, is the same number of days.
        #[test]
        fn days_round_trip(days in -1_000_000i64..1_000_000) {
            let (y, m, d) = civil_from_days(days);
            prop_assert!((1..=12).contains(&m));
            prop_assert!((1..=31).contains(&d));
            // Inverse: days_from_civil.
            let y2 = if m <= 2 { y - 1 } else { y };
            let era = y2.div_euclid(400);
            let yoe = y2.rem_euclid(400);
            let mp = i64::from((m + 9) % 12);
            let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
            let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
            prop_assert_eq!(era * 146_097 + doe - 719_468, days);
        }
    }
}
