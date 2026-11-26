use chrono::{DateTime, NaiveDate};

const FIVE_MINUTES_MS: f64 = 5.0 * 60_000.0;
const ONE_YEAR_MS: f64 = 365.0 * 24.0 * 3_600_000.0;

/// Resolves which UTC day a listen belongs to.
///
/// Listens recorded offline are replayed later carrying their original
/// timestamp, so the client controls this value and it cannot be trusted: a wrong
/// clock (or a hostile caller) would otherwise write rows years into the future
/// and skew every chart.
pub fn listen_day(at: Option<f64>, now_ms: i64) -> NaiveDate {
    let now = now_ms as f64;
    let claimed = at.filter(|value| value.is_finite()).unwrap_or(now);
    let when = if claimed > now + FIVE_MINUTES_MS || claimed < now - ONE_YEAR_MS {
        now
    } else {
        claimed
    };

    DateTime::from_timestamp_millis(when as i64)
        .expect("bounded to within a year of now")
        .date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn now() -> i64 {
        Utc.with_ymd_and_hms(2026, 7, 30, 14, 30, 0)
            .unwrap()
            .timestamp_millis()
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, d).unwrap()
    }

    const DAY_MS: f64 = 24.0 * 3_600_000.0;

    #[test]
    fn truncates_to_the_utc_day() {
        assert_eq!(listen_day(Some(now() as f64), now()), day(30));
    }

    #[test]
    fn keeps_an_offline_listen_on_the_day_it_happened() {
        assert_eq!(
            listen_day(Some(now() as f64 - 3.0 * DAY_MS), now()),
            day(27)
        );
    }

    #[test]
    fn falls_back_to_now_when_no_timestamp_is_given() {
        assert_eq!(listen_day(None, now()), day(30));
    }

    #[test]
    fn rejects_a_timestamp_from_the_future() {
        assert_eq!(
            listen_day(Some(now() as f64 + 365.0 * DAY_MS), now()),
            day(30)
        );
    }

    #[test]
    fn rejects_a_timestamp_older_than_a_year() {
        assert_eq!(
            listen_day(Some(now() as f64 - 400.0 * DAY_MS), now()),
            day(30)
        );
    }

    #[test]
    fn allows_small_clock_skew() {
        assert_eq!(listen_day(Some(now() as f64 + 60_000.0), now()), day(30));
    }

    #[test]
    fn ignores_non_finite_input() {
        for bad in [f64::NAN, f64::INFINITY] {
            assert_eq!(listen_day(Some(bad), now()), day(30));
        }
    }
}
