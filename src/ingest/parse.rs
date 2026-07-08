//! feed-rs → normalized items (prompt.md §5, §11 "Feeds & parsing"). Handles RSS 2.0 / RSS 1.0
//! RDF / Atom / JSON Feed uniformly, with the date fallback and content selection the spec wants.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use feed_rs::model::{Entry, Feed};

use super::settings::IngestSettings;
use super::{content, dedup_key, FeedKind, ParsedFeed, ParsedItem};

/// Parse a feed body. `kind_hint` is the stored feed kind (youtube/reddit are pre-classified);
/// generic feeds keep their hint (rss/atom/jsonfeed are cosmetic once parsed).
pub fn parse_feed(
    bytes: &[u8],
    feed_url: &str,
    kind_hint: FeedKind,
    cfg: &IngestSettings,
    fetched_at: DateTime<Utc>,
) -> Result<ParsedFeed> {
    let feed = feed_rs::parser::parse(bytes).context("could not parse feed (unsupported/malformed)")?;

    let title = feed.title.as_ref().map(|t| t.content.trim().to_string()).filter(|s| !s.is_empty());
    let site_url = pick_site_link(&feed).or_else(|| Some(feed_url.to_string()));
    let icon_url = feed
        .icon
        .as_ref()
        .map(|i| i.uri.clone())
        .or_else(|| feed.logo.as_ref().map(|i| i.uri.clone()));

    let is_video = kind_hint == FeedKind::Youtube;
    let items = feed
        .entries
        .iter()
        .map(|e| parse_entry(e, feed_url, is_video, cfg, fetched_at))
        .collect();

    Ok(ParsedFeed { title, site_url, icon_url, items })
}

fn parse_entry(
    e: &Entry,
    feed_url: &str,
    is_video: bool,
    cfg: &IngestSettings,
    fetched_at: DateTime<Utc>,
) -> ParsedItem {
    let url = pick_entry_link(e).or_else(|| Some(feed_url.to_string()));
    let base = url.as_deref().unwrap_or(feed_url);

    let title = e.title.as_ref().map(|t| t.content.trim().to_string()).filter(|s| !s.is_empty());
    let author = e
        .authors
        .first()
        .map(|p| p.name.clone())
        .filter(|s| !s.is_empty());

    // Content: prefer full <content>, else the summary/description.
    let raw_html = e
        .content
        .as_ref()
        .and_then(|c| c.body.clone())
        .or_else(|| e.summary.as_ref().map(|s| s.content.clone()))
        // YouTube: description often lives on the media object.
        .or_else(|| {
            e.media
                .iter()
                .find_map(|m| m.description.as_ref().map(|d| d.content.clone()))
        })
        .unwrap_or_default();

    let content_html = content::sanitize_html(&raw_html, Some(base), cfg.item_content_cap);
    let content_text = content::to_text(&content_html, cfg.item_content_cap);

    let image_url = media_image(e).or_else(|| content::first_image(&content_html));

    // Date fallback: published → updated → fetched_at (never null; ordering needs it, §11).
    let published_at = e.published.or(e.updated).unwrap_or(fetched_at);

    let duration_secs = if is_video {
        e.media.iter().find_map(|m| m.duration.map(|d| d.as_secs() as i64))
    } else {
        None
    };

    let reading_time_secs = Some(content::reading_time_secs(&content_text));

    let guid = Some(e.id.clone()).filter(|g| !g.trim().is_empty());
    let dedup_hash = dedup_key(guid.as_deref(), url.as_deref(), title.as_deref(), Some(&content_text));

    ParsedItem {
        guid,
        url,
        title,
        author,
        content_html: Some(content_html).filter(|s| !s.is_empty()),
        content_text: Some(content_text).filter(|s| !s.is_empty()),
        image_url,
        duration_secs,
        reading_time_secs,
        published_at,
        score: None,
        comments_count: None,
        upvote_ratio: None,
        dedup_hash,
    }
}

/// Lead image from MediaRSS: thumbnail first (YouTube uses these), then a media content URL.
fn media_image(e: &Entry) -> Option<String> {
    for m in &e.media {
        if let Some(t) = m.thumbnails.first() {
            return Some(t.image.uri.clone());
        }
        if let Some(c) = m.content.iter().find(|c| c.url.is_some()) {
            return c.url.as_ref().map(|u| u.to_string());
        }
    }
    None
}

fn pick_entry_link(e: &Entry) -> Option<String> {
    e.links
        .iter()
        .find(|l| l.rel.as_deref() == Some("alternate"))
        .or_else(|| e.links.iter().find(|l| l.href.starts_with("http")))
        .map(|l| l.href.clone())
}

fn pick_site_link(feed: &Feed) -> Option<String> {
    feed.links
        .iter()
        .find(|l| l.rel.as_deref() == Some("alternate"))
        .or_else(|| feed.links.iter().find(|l| l.href.starts_with("http")))
        .map(|l| l.href.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
    <rss version="2.0"><channel>
      <title>Example</title><link>https://ex.com</link>
      <item>
        <title>First</title><link>https://ex.com/1</link>
        <guid>guid-1</guid>
        <description>&lt;p&gt;Hello &lt;script&gt;alert(1)&lt;/script&gt; world&lt;/p&gt;</description>
        <pubDate>Tue, 10 Jun 2025 09:00:00 GMT</pubDate>
      </item>
      <item>
        <title>No date</title><link>https://ex.com/2</link>
      </item>
    </channel></rss>"#;

    #[test]
    fn parses_rss_with_sanitize_and_date_fallback() {
        let cfg = IngestSettings::default();
        let now = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
        let feed = parse_feed(RSS.as_bytes(), "https://ex.com/rss", FeedKind::Rss, &cfg, now).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Example"));
        assert_eq!(feed.items.len(), 2);

        let first = &feed.items[0];
        assert_eq!(first.title.as_deref(), Some("First"));
        assert!(!first.content_html.as_ref().unwrap().contains("script"));
        assert_eq!(first.guid.as_deref(), Some("guid-1"));
        assert!(first.reading_time_secs.unwrap() >= 30);

        // Second item has no pubDate → falls back to fetched_at (not null).
        assert_eq!(feed.items[1].published_at, now);
    }
}
