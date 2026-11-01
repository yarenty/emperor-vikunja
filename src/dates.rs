//! Dates in and out of Vikunja.
//!
//! Vikunja speaks RFC 3339 and marks "no date" with the zero time `0001-01-01T00:00:00Z`.
//! Tools accept a full RFC 3339 timestamp or a plain `YYYY-MM-DD` day (read as 00:00 UTC) and
//! report dates as RFC 3339, or `null` when unset.

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};

/// Parse a date argument: `YYYY-MM-DD` (00:00 UTC that day) or an RFC 3339 timestamp.
pub fn parse(input: &str) -> Result<DateTime<Utc>, String> {
    let s = input.trim();
    if let Ok(day) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let midnight = day.and_hms_opt(0, 0, 0).expect("midnight exists");
        return Ok(midnight.and_utc());
    }
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| format!("`{s}` is not a date; use YYYY-MM-DD or an ISO-8601 timestamp"))
}

/// Format a timestamp the way Vikunja expects it.
pub fn format(d: DateTime<Utc>) -> String {
    d.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// A date as Vikunja returned it, or `None` when it is empty or the zero time.
pub fn present(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if s.is_empty() || s.starts_with("0001-") {
        None
    } else {
        Some(s.to_string())
    }
}

/// [`present`] parsed into a timestamp (for sorting and comparing).
pub fn parsed(raw: Option<&str>) -> Option<DateTime<Utc>> {
    present(raw).and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_is_midnight_utc() {
        assert_eq!(format(parse("2026-11-05").unwrap()), "2026-11-05T00:00:00Z");
    }

    #[test]
    fn rfc3339_is_normalised_to_utc() {
        assert_eq!(
            format(parse("2026-11-05T10:30:00+01:00").unwrap()),
            "2026-11-05T09:30:00Z"
        );
    }

    #[test]
    fn garbage_is_refused() {
        assert!(parse("next tuesday").unwrap_err().contains("YYYY-MM-DD"));
    }

    #[test]
    fn zero_time_means_unset() {
        assert_eq!(present(Some("0001-01-01T00:00:00Z")), None);
        assert_eq!(present(None), None);
        assert_eq!(present(Some("")), None);
        assert_eq!(
            present(Some("2026-11-05T00:00:00Z")).as_deref(),
            Some("2026-11-05T00:00:00Z")
        );
    }
}
