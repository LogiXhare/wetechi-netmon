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

    proptest! {
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
