//! Shared ingestion engine (prompt.md §4, §5) - the heart of the app. Feeds are polled **once**
//! for all users by a background `tokio` scheduler; one bad feed never crashes the loop.
//!
//! Layout: `fetch` (HTTP + conditional GET + redirects + caps), `parse` (feed-rs → items),
//! `reddit` (JSON for score/comments), `content` (sanitize/text/image/reading-time), `store`
//! (dedup + transactional insert + backoff), `scheduler` (the loop), `discover` (URL → feeds).

pub mod content;
pub mod discover;
pub mod fetch;
pub mod parse;
pub mod reddit;
pub mod scheduler;
pub mod settings;
pub mod store;
pub mod url_util;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

pub use scheduler::{spawn, IngestTrigger};

/// Feed source kind - matches the `feeds.kind` CHECK constraint (prompt.md §2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeedKind {
    Rss,
    Atom,
    JsonFeed,
    Youtube,
    Reddit,
}

impl FeedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FeedKind::Rss => "rss",
            FeedKind::Atom => "atom",
            FeedKind::JsonFeed => "jsonfeed",
            FeedKind::Youtube => "youtube",
            FeedKind::Reddit => "reddit",
        }
    }

    pub fn from_db(s: &str) -> FeedKind {
        match s {
            "atom" => FeedKind::Atom,
            "jsonfeed" => FeedKind::JsonFeed,
            "youtube" => FeedKind::Youtube,
            "reddit" => FeedKind::Reddit,
            _ => FeedKind::Rss,
        }
    }

    /// Default per-subscription content type (prompt.md §2): YouTube → video, else reading.
    pub fn default_content_type(self) -> &'static str {
        match self {
            FeedKind::Youtube => "video",
            _ => "reading",
        }
    }
}

/// Feed-level metadata + items produced by a parser.
pub struct ParsedFeed {
    pub title: Option<String>,
    pub site_url: Option<String>,
    pub icon_url: Option<String>,
    pub items: Vec<ParsedItem>,
}

/// A normalized, sanitized item ready to store. `published_at` is always set (date fallback).
pub struct ParsedItem {
    pub guid: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub content_html: Option<String>,
    pub content_text: Option<String>,
    pub image_url: Option<String>,
    pub duration_secs: Option<i64>,
    pub reading_time_secs: Option<i64>,
    pub published_at: DateTime<Utc>,
    pub score: Option<i64>,
    pub comments_count: Option<i64>,
    pub upvote_ratio: Option<f64>,
    pub dedup_hash: String,
}

/// Build the dedup key (prompt.md §11): prefer GUID, then normalized URL, then a hash of
/// title+content so GUID-less feeds still dedupe.
pub fn dedup_key(
    guid: Option<&str>,
    url: Option<&str>,
    title: Option<&str>,
    text: Option<&str>,
) -> String {
    if let Some(g) = guid.map(str::trim).filter(|g| !g.is_empty()) {
        return format!("guid:{g}");
    }
    if let Some(u) = url.map(str::trim).filter(|u| !u.is_empty()) {
        let key = url_util::normalize_url(u).unwrap_or_else(|| u.to_string());
        return format!("url:{key}");
    }
    let mut h = Sha256::new();
    h.update(title.unwrap_or("").as_bytes());
    h.update(b"\0");
    h.update(text.unwrap_or("").as_bytes());
    format!("hash:{:x}", h.finalize())
}

/// Exponential backoff with a ~6h cap plus up to +25% jitter (prompt.md §4). `jitter_frac` is a
/// caller-supplied `[0,1)` value (random in production, fixed in tests).
pub fn backoff_secs(failure_count: i64, jitter_frac: f64) -> i64 {
    let base = 60i64;
    let exp = (failure_count.clamp(1, 24) - 1) as u32;
    let raw = base.saturating_mul(1i64.checked_shl(exp).unwrap_or(i64::MAX));
    let capped = raw.min(settings::BACKOFF_CAP_SECS);
    let jitter = (capped as f64 * 0.25 * jitter_frac.clamp(0.0, 1.0)) as i64;
    capped.saturating_add(jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_prefers_guid_then_url_then_hash() {
        assert_eq!(
            dedup_key(Some("abc"), Some("http://x"), None, None),
            "guid:abc"
        );
        assert!(dedup_key(None, Some("https://x.com/a/"), None, None)
            .starts_with("url:https://x.com/a"));
        assert!(dedup_key(None, None, Some("t"), Some("body")).starts_with("hash:"));
        // Same title+content → same hash (stable dedup for GUID-less feeds).
        assert_eq!(
            dedup_key(None, None, Some("t"), Some("b")),
            dedup_key(None, None, Some("t"), Some("b"))
        );
    }

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff_secs(1, 0.0), 60);
        assert_eq!(backoff_secs(2, 0.0), 120);
        assert_eq!(backoff_secs(3, 0.0), 240);
        assert_eq!(backoff_secs(100, 0.0), settings::BACKOFF_CAP_SECS);
        // Jitter only adds, never below the base.
        assert!(backoff_secs(2, 0.99) >= 120);
        assert!(backoff_secs(2, 0.99) <= 120 + 30);
    }
}
