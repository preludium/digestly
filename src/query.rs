//! Pure helpers for the items query (prompt.md §10, §11): timezone-correct `when` ranges,
//! whitelisted sort clauses (NULLs-last for metric sorts), and FTS query construction.
//!
//! These are the classic silent bugs (per the phase risk notes), so the logic lives here as
//! side-effect-free functions with unit tests - the handlers just consume them.

use std::str::FromStr;

use chrono::{DateTime, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

/// A half-open published-at range `[start, end)` as stored-timestamp strings
/// (`"%Y-%m-%d %H:%M:%S"`, UTC), matching how ingestion writes `items.published_at`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WhenRange {
    pub start: Option<String>,
    pub end: Option<String>,
}

/// Parse a stored timezone string into a `Tz`, defaulting to UTC on anything unknown/empty.
pub fn parse_tz(value: Option<&str>) -> Tz {
    value
        .and_then(|v| Tz::from_str(v).ok())
        .unwrap_or(chrono_tz::UTC)
}

fn fmt(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Local midnight (`date` at 00:00 in `tz`) as a UTC instant - DST-safe.
///
/// Uses the wall-clock date in the user's zone, so "today" tracks the real offset (EST vs EDT),
/// never a fixed UTC day boundary. Handles the rare spring-forward gap where 00:00 doesn't exist.
fn local_midnight_utc(tz: &Tz, date: NaiveDate) -> DateTime<Utc> {
    let naive = date.and_hms_opt(0, 0, 0).expect("00:00:00 is always valid");
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        // Fall-back (ambiguous): pick the earlier instant - the range still covers the whole day.
        LocalResult::Ambiguous(earlier, _) => earlier.with_timezone(&Utc),
        // Gap (midnight skipped by a forward transition): step to the first valid wall-clock time.
        LocalResult::None => {
            let mut probe = naive;
            for _ in 0..6 {
                probe += Duration::hours(1);
                if let LocalResult::Single(dt) = tz.from_local_datetime(&probe) {
                    return dt.with_timezone(&Utc);
                }
            }
            Utc.from_utc_datetime(&naive)
        }
    }
}

/// Compute the UTC bounds for a `when` facet in the user's timezone (prompt.md §11).
///
/// `today`/`yesterday` are calendar days in `tz`; `24h`/`week`/`month` are rolling windows (last
/// 24 hours/7/30 days from `now`, not calendar-aligned). `all` (or anything unknown) yields an
/// unbounded range.
pub fn when_range(when: &str, tz: &Tz, now: DateTime<Utc>) -> WhenRange {
    let today = now.with_timezone(tz).date_naive();
    match when {
        "today" => {
            let start = local_midnight_utc(tz, today);
            let end = local_midnight_utc(tz, today + Duration::days(1));
            WhenRange {
                start: Some(fmt(start)),
                end: Some(fmt(end)),
            }
        }
        "yesterday" => {
            let start = local_midnight_utc(tz, today - Duration::days(1));
            let end = local_midnight_utc(tz, today);
            WhenRange {
                start: Some(fmt(start)),
                end: Some(fmt(end)),
            }
        }
        "24h" => WhenRange {
            start: Some(fmt(now - Duration::hours(24))),
            end: None,
        },
        "week" => WhenRange {
            start: Some(fmt(now - Duration::days(7))),
            end: None,
        },
        "month" => WhenRange {
            start: Some(fmt(now - Duration::days(30))),
            end: None,
        },
        _ => WhenRange::default(),
    }
}

/// Whitelisted `ORDER BY` expression for a sort key (prompt.md §9.1, §11). Returned as a
/// `&'static str` so it is safe to interpolate directly (never user input). Metric sorts push
/// NULLs last so metric-less items don't dominate.
///
/// Relies on the query aliasing `items` as `i` and `LEFT JOIN item_states` as `st`.
pub fn sort_clause(sort: &str) -> &'static str {
    match sort {
        "old" => "i.published_at ASC",
        "quick" => "i.reading_time_secs IS NULL, i.reading_time_secs ASC, i.published_at DESC",
        "top" => "i.score IS NULL, i.score DESC, i.published_at DESC",
        "discussed" => "i.comments_count IS NULL, i.comments_count DESC, i.published_at DESC",
        "unread" => "COALESCE(st.is_read, 0) ASC, i.published_at DESC",
        // "new" and any unknown value.
        _ => "i.published_at DESC",
    }
}

/// Build a safe FTS5 MATCH expression from raw user input (prompt.md §9.2). Each whitespace token
/// is reduced to alphanumerics and turned into a quoted prefix term, so arbitrary punctuation can
/// never produce an FTS syntax error. Returns `None` when nothing searchable remains.
pub fn fts_query(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split_whitespace()
        .map(|t| {
            t.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
            .unwrap()
            .and_utc()
    }

    #[test]
    fn when_all_is_unbounded() {
        let r = when_range("all", &chrono_tz::UTC, utc("2021-06-15 12:00:00"));
        assert_eq!(r, WhenRange::default());
        // Unknown values behave like "all".
        assert_eq!(
            when_range("garbage", &chrono_tz::UTC, utc("2021-06-15 12:00:00")),
            WhenRange::default()
        );
    }

    #[test]
    fn today_respects_non_utc_timezone_and_dst() {
        let ny: Tz = "America/New_York".parse().unwrap();

        // Summer (EDT, UTC-4): the NY calendar day for 2021-07-15 12:00 UTC is 2021-07-15,
        // whose local midnight is 04:00 UTC.
        let summer = when_range("today", &ny, utc("2021-07-15 12:00:00"));
        assert_eq!(summer.start.as_deref(), Some("2021-07-15 04:00:00"));
        assert_eq!(summer.end.as_deref(), Some("2021-07-16 04:00:00"));

        // Winter (EST, UTC-5): same wall time, but local midnight is now 05:00 UTC - proving the
        // boundary tracks the real offset, not a fixed UTC day.
        let winter = when_range("today", &ny, utc("2021-01-15 12:00:00"));
        assert_eq!(winter.start.as_deref(), Some("2021-01-15 05:00:00"));
        assert_eq!(winter.end.as_deref(), Some("2021-01-16 05:00:00"));
    }

    #[test]
    fn today_crosses_a_dst_spring_forward_boundary() {
        // US DST began 2021-03-14 02:00 (EST→EDT). At 2021-03-14 12:00 UTC it's still the 14th
        // in NY; that day's midnight is 05:00 UTC (still EST), and the NEXT midnight is 04:00 UTC
        // (now EDT) - i.e. the day is only 23h wide. The range must reflect that.
        let ny: Tz = "America/New_York".parse().unwrap();
        let r = when_range("today", &ny, utc("2021-03-14 12:00:00"));
        assert_eq!(r.start.as_deref(), Some("2021-03-14 05:00:00"));
        assert_eq!(r.end.as_deref(), Some("2021-03-15 04:00:00"));
    }

    #[test]
    fn yesterday_is_the_previous_calendar_day() {
        let ny: Tz = "America/New_York".parse().unwrap();
        let r = when_range("yesterday", &ny, utc("2021-07-15 12:00:00"));
        assert_eq!(r.start.as_deref(), Some("2021-07-14 04:00:00"));
        assert_eq!(r.end.as_deref(), Some("2021-07-15 04:00:00"));
    }

    #[test]
    fn week_and_month_are_rolling_windows() {
        let now = utc("2021-06-15 12:00:00");
        assert_eq!(
            when_range("week", &chrono_tz::UTC, now).start.as_deref(),
            Some("2021-06-08 12:00:00")
        );
        assert_eq!(
            when_range("month", &chrono_tz::UTC, now).start.as_deref(),
            Some("2021-05-16 12:00:00")
        );
        assert!(when_range("week", &chrono_tz::UTC, now).end.is_none());
    }

    #[test]
    fn last_24h_is_a_rolling_window_not_a_calendar_day() {
        let now = utc("2021-06-15 12:00:00");
        let r = when_range("24h", &chrono_tz::UTC, now);
        assert_eq!(r.start.as_deref(), Some("2021-06-14 12:00:00"));
        assert!(r.end.is_none());
    }

    #[test]
    fn metric_sorts_put_nulls_last() {
        assert!(sort_clause("top").starts_with("i.score IS NULL"));
        assert!(sort_clause("discussed").starts_with("i.comments_count IS NULL"));
        assert!(sort_clause("quick").starts_with("i.reading_time_secs IS NULL"));
        assert_eq!(sort_clause("new"), "i.published_at DESC");
        assert_eq!(sort_clause("old"), "i.published_at ASC");
        // Unknown falls back to newest-first.
        assert_eq!(sort_clause("nonsense"), "i.published_at DESC");
    }

    #[test]
    fn fts_query_sanitizes_punctuation_into_prefix_terms() {
        assert_eq!(
            fts_query("rust async").as_deref(),
            Some("\"rust\"* \"async\"*")
        );
        // Punctuation around a word is stripped so it can't break FTS parsing.
        assert_eq!(
            fts_query("rust, async!").as_deref(),
            Some("\"rust\"* \"async\"*")
        );
        // Embedded punctuation collapses the token (lossy but safe - never an FTS operator).
        assert_eq!(
            fts_query("  NEAR(\"x\") OR *").as_deref(),
            Some("\"NEARx\"* \"OR\"*")
        );
        assert_eq!(fts_query("   ").as_deref(), None);
        assert_eq!(fts_query("!@#$%").as_deref(), None);
    }
}
