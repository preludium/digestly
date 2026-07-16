//! A small, dependency-free cron parser for the digest schedule (prompt.md §7 "schedule cron",
//! §9.7 "cron with human preview"). Standard 5-field format
//! `minute hour day-of-month month day-of-week`, supporting `*`, single values, `a-b` ranges,
//! `a,b,c` lists, and `*/step`. Day-of-week is 0–6 with Sunday = 0 (7 also accepted for Sunday).
//!
//! Matching is done against **local** time in the configured timezone, so the schedule is
//! DST-correct (chrono-tz resolves the wall-clock fields, §11 "DST-correct cron").

use chrono::{Datelike, Timelike};

/// A parsed cron expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Cron {
    minute: Field,
    hour: Field,
    dom: Field,
    month: Field,
    dow: Field,
}

#[derive(Debug, Clone, PartialEq)]
enum Field {
    Any,
    Values(Vec<u32>),
}

impl Cron {
    /// Parse a 5-field cron string. Returns `None` on any malformed field.
    pub fn parse(expr: &str) -> Option<Cron> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return None;
        }
        Some(Cron {
            minute: Field::parse(parts[0], 0, 59)?,
            hour: Field::parse(parts[1], 0, 23)?,
            dom: Field::parse(parts[2], 1, 31)?,
            month: Field::parse(parts[3], 1, 12)?,
            dow: Field::parse_dow(parts[4])?,
        })
    }

    /// True when `dt` (already expressed in the target timezone) matches the schedule for its
    /// minute. Day-of-month and day-of-week use the standard cron OR-semantics when both are
    /// restricted.
    pub fn matches<Tz: chrono::TimeZone>(&self, dt: &chrono::DateTime<Tz>) -> bool {
        let minute = dt.minute();
        let hour = dt.hour();
        let dom = dt.day();
        let month = dt.month();
        // chrono weekday: Mon=0..Sun=6 via num_days_from_monday; convert to cron Sun=0..Sat=6.
        let dow = dt.weekday().num_days_from_sunday();

        if !self.minute.contains(minute) || !self.hour.contains(hour) || !self.month.contains(month)
        {
            return false;
        }
        match (&self.dom, &self.dow) {
            (Field::Any, Field::Any) => true,
            (dom_f, Field::Any) => dom_f.contains(dom),
            (Field::Any, dow_f) => dow_f.contains(dow),
            // Both restricted → match if EITHER matches (standard cron).
            (dom_f, dow_f) => dom_f.contains(dom) || dow_f.contains(dow),
        }
    }

    /// A best-effort human description for the UI ("Every Monday at 09:00"), falling back to a
    /// generic phrasing for expressions this simple describer doesn't specialise.
    pub fn describe(&self) -> String {
        let time = match (&self.minute, &self.hour) {
            (Field::Values(m), Field::Values(h)) if m.len() == 1 && h.len() == 1 => {
                format!("{:02}:{:02}", h[0], m[0])
            }
            _ => return format!("On schedule ({})", self.summary_fallback()),
        };
        // Only specialise the common "day selector" shapes.
        if self.month != Field::Any {
            return format!("On schedule ({})", self.summary_fallback());
        }
        match (&self.dom, &self.dow) {
            (Field::Any, Field::Any) => format!("Every day at {time}"),
            (Field::Any, Field::Values(days)) => {
                let names: Vec<&str> = days.iter().map(|d| weekday_name(*d)).collect();
                format!("Every {} at {time}", names.join(", "))
            }
            (Field::Values(doms), Field::Any) if doms.len() == 1 => {
                format!("On day {} of each month at {time}", doms[0])
            }
            _ => format!("On schedule ({})", self.summary_fallback()),
        }
    }

    fn summary_fallback(&self) -> String {
        "custom cron".to_string()
    }

    /// The next minute (strictly after `after`) at which this schedule matches. Searches forward
    /// minute-by-minute, capped at ~2 years so a schedule that can never match (e.g. day-of-month
    /// 31 combined with a month restriction that never has one) returns `None` instead of looping
    /// forever.
    pub fn next_after<Tz: chrono::TimeZone>(
        &self,
        after: &chrono::DateTime<Tz>,
    ) -> Option<chrono::DateTime<Tz>> {
        const MAX_MINUTES: i64 = 366 * 24 * 60 * 2;
        let mut candidate = after.clone() + chrono::Duration::minutes(1);
        candidate -= chrono::Duration::seconds(candidate.second() as i64)
            + chrono::Duration::nanoseconds(candidate.nanosecond() as i64);
        for _ in 0..MAX_MINUTES {
            if self.matches(&candidate) {
                return Some(candidate);
            }
            candidate += chrono::Duration::minutes(1);
        }
        None
    }
}

impl Field {
    fn parse(spec: &str, min: u32, max: u32) -> Option<Field> {
        if spec == "*" {
            return Some(Field::Any);
        }
        let mut values: Vec<u32> = Vec::new();
        for part in spec.split(',') {
            // `*/step` or `a-b/step`
            let (range_part, step) = match part.split_once('/') {
                Some((r, s)) => (r, s.parse::<u32>().ok().filter(|s| *s > 0)?),
                None => (part, 1),
            };
            let (lo, hi) = if range_part == "*" {
                (min, max)
            } else if let Some((a, b)) = range_part.split_once('-') {
                (a.parse().ok()?, b.parse().ok()?)
            } else {
                let v: u32 = range_part.parse().ok()?;
                (v, v)
            };
            if lo > hi || lo < min || hi > max {
                return None;
            }
            let mut v = lo;
            while v <= hi {
                values.push(v);
                v += step;
            }
        }
        if values.is_empty() {
            return None;
        }
        values.sort_unstable();
        values.dedup();
        Some(Field::Values(values))
    }

    /// Day-of-week accepts 0–7 (both 0 and 7 mean Sunday), normalised to 0–6.
    fn parse_dow(spec: &str) -> Option<Field> {
        let f = Field::parse(spec, 0, 7)?;
        Some(match f {
            Field::Any => Field::Any,
            Field::Values(vs) => {
                let mut norm: Vec<u32> =
                    vs.into_iter().map(|v| if v == 7 { 0 } else { v }).collect();
                norm.sort_unstable();
                norm.dedup();
                Field::Values(norm)
            }
        })
    }

    fn contains(&self, v: u32) -> bool {
        match self {
            Field::Any => true,
            Field::Values(vs) => vs.contains(&v),
        }
    }
}

fn weekday_name(dow: u32) -> &'static str {
    match dow {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        6 => "Saturday",
        _ => "day",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Tz;

    fn at(tz: Tz, y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<Tz> {
        tz.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn parses_and_matches_weekly_monday_9am() {
        let c = Cron::parse("0 9 * * 1").unwrap();
        // 2024-01-01 was a Monday.
        assert!(c.matches(&at(Tz::UTC, 2024, 1, 1, 9, 0)));
        assert!(!c.matches(&at(Tz::UTC, 2024, 1, 1, 9, 1)), "wrong minute");
        assert!(
            !c.matches(&at(Tz::UTC, 2024, 1, 2, 9, 0)),
            "Tuesday, not Monday"
        );
        assert_eq!(c.describe(), "Every Monday at 09:00");
    }

    #[test]
    fn daily_and_lists_and_steps() {
        assert!(Cron::parse("30 6 * * *")
            .unwrap()
            .matches(&at(Tz::UTC, 2024, 3, 10, 6, 30)));
        let weekdays = Cron::parse("0 8 * * 1-5").unwrap();
        assert!(
            weekdays.matches(&at(Tz::UTC, 2024, 1, 1, 8, 0)),
            "Monday in 1-5"
        );
        assert!(
            !weekdays.matches(&at(Tz::UTC, 2024, 1, 6, 8, 0)),
            "Saturday not in 1-5"
        );
        let every15 = Cron::parse("*/15 * * * *").unwrap();
        assert!(every15.matches(&at(Tz::UTC, 2024, 1, 1, 3, 45)));
        assert!(!every15.matches(&at(Tz::UTC, 2024, 1, 1, 3, 46)));
    }

    #[test]
    fn sunday_is_zero_or_seven() {
        let sun0 = Cron::parse("0 9 * * 0").unwrap();
        let sun7 = Cron::parse("0 9 * * 7").unwrap();
        // 2024-01-07 was a Sunday.
        assert!(sun0.matches(&at(Tz::UTC, 2024, 1, 7, 9, 0)));
        assert!(sun7.matches(&at(Tz::UTC, 2024, 1, 7, 9, 0)));
        assert_eq!(sun0, sun7, "7 normalises to 0");
    }

    #[test]
    fn dst_correct_local_time() {
        // US spring-forward 2024-03-10 02:00 → 03:00 in America/New_York. A 09:00 daily schedule
        // still fires at local 09:00 that day (matching is on wall-clock local fields).
        let ny: Tz = "America/New_York".parse().unwrap();
        let c = Cron::parse("0 9 * * *").unwrap();
        assert!(c.matches(&at(ny, 2024, 3, 10, 9, 0)));
    }

    #[test]
    fn next_after_finds_the_following_occurrence() {
        let daily = Cron::parse("0 5 * * *").unwrap();
        // Same day, before the fire time → later today.
        assert_eq!(
            daily.next_after(&at(Tz::UTC, 2024, 1, 1, 4, 30)),
            Some(at(Tz::UTC, 2024, 1, 1, 5, 0))
        );
        // Exactly at the fire time → "next" is strictly after, so tomorrow.
        assert_eq!(
            daily.next_after(&at(Tz::UTC, 2024, 1, 1, 5, 0)),
            Some(at(Tz::UTC, 2024, 1, 2, 5, 0))
        );
        // Sub-minute precision on `after` is truncated away, not skipped past.
        let with_seconds = at(Tz::UTC, 2024, 1, 1, 4, 59) + chrono::Duration::seconds(30);
        assert_eq!(
            daily.next_after(&with_seconds),
            Some(at(Tz::UTC, 2024, 1, 1, 5, 0))
        );

        let weekly_mon = Cron::parse("0 9 * * 1").unwrap();
        // 2024-01-01 is a Monday; asking right after that fire jumps a full week forward.
        assert_eq!(
            weekly_mon.next_after(&at(Tz::UTC, 2024, 1, 1, 9, 0)),
            Some(at(Tz::UTC, 2024, 1, 8, 9, 0))
        );
    }

    #[test]
    fn rejects_malformed() {
        assert!(Cron::parse("").is_none());
        assert!(Cron::parse("0 9 * *").is_none(), "only 4 fields");
        assert!(Cron::parse("60 9 * * 1").is_none(), "minute out of range");
        assert!(Cron::parse("0 24 * * 1").is_none(), "hour out of range");
        assert!(Cron::parse("a b c d e").is_none());
    }
}
