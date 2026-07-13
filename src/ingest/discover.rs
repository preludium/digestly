//! Feed discovery (prompt.md §9.3, §11 "Discovery"): turn a site URL, feed URL, YouTube
//! channel/@handle, or subreddit into concrete feed candidates. `<link rel=alternate>` sniffing,
//! common paths, YouTube per-channel RSS + handle→id resolution, Reddit special-casing.

use anyhow::Result;
use reqwest::Client;
use serde::Serialize;

use super::fetch::{self, Conditional, FetchOutcome};
use super::settings::IngestSettings;
use super::{reddit, url_util, FeedKind};

/// A discovered feed the user can subscribe to.
#[derive(Serialize, Clone, Debug)]
pub struct Candidate {
    pub feed_url: String,
    pub title: Option<String>,
    pub kind: String,
    pub site_url: Option<String>,
    pub icon_url: Option<String>,
}

/// Common feed paths probed on a bare site URL.
const COMMON_PATHS: [&str; 6] = [
    "/feed",
    "/rss",
    "/rss.xml",
    "/atom.xml",
    "/feed.json",
    "/index.xml",
];

/// Discover feed candidates from arbitrary user input. Returns `Ok(vec![])` when nothing is found
/// (the UI shows a "none found - enter a feed URL directly" state).
pub async fn discover(
    client: &Client,
    cfg: &IngestSettings,
    input: &str,
) -> Result<Vec<Candidate>> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(vec![]);
    }

    // Reddit: subreddit name or any reddit.com/r/<sub> URL.
    if let Some(sub) = reddit_subreddit(input) {
        let feed_url = url_util::normalize_url(&reddit::rss_url(&sub))
            .unwrap_or_else(|| reddit::rss_url(&sub));
        return Ok(vec![Candidate {
            feed_url,
            title: Some(format!("r/{sub}")),
            kind: FeedKind::Reddit.as_str().to_string(),
            site_url: Some(format!("https://www.reddit.com/r/{sub}")),
            icon_url: None,
        }]);
    }

    // YouTube: @handle, channel_id, or any youtube.com URL.
    if let Some(candidate) = youtube_candidate(client, cfg, input).await? {
        return Ok(vec![candidate]);
    }

    // Otherwise it must be an http(s) URL - a direct feed or an HTML page to sniff.
    let url = match url_util::guard_public_url(input, cfg.allow_private) {
        Ok(u) => u,
        Err(_) => return Ok(vec![]),
    };
    discover_from_url(client, cfg, &url).await
}

async fn discover_from_url(
    client: &Client,
    cfg: &IngestSettings,
    url: &str,
) -> Result<Vec<Candidate>> {
    let body = match fetch_body(client, cfg, url).await {
        Some(b) => b,
        None => return Ok(vec![]),
    };

    // Is the URL itself a feed?
    if let Some(kind) = feed_kind_of(&body) {
        return Ok(vec![feed_candidate(&body, url, kind)]);
    }

    // Treat as HTML: sniff <link rel=alternate> then probe common paths.
    let mut candidates = sniff_alternate_links(&body, url);
    if candidates.is_empty() {
        for path in COMMON_PATHS {
            if let Some(feed_url) = url_util::resolve(url, path) {
                if let Some(b) = fetch_body(client, cfg, &feed_url).await {
                    if let Some(kind) = feed_kind_of(&b) {
                        candidates.push(feed_candidate(&b, &feed_url, kind));
                    }
                }
            }
        }
    }
    // Dedupe candidates by normalized feed_url.
    candidates.sort_by(|a, b| a.feed_url.cmp(&b.feed_url));
    candidates.dedup_by(|a, b| a.feed_url == b.feed_url);
    Ok(candidates)
}

/// Detect a subreddit from `r/name`, `/r/name`, or a reddit.com URL.
fn reddit_subreddit(input: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    if lower.contains("reddit.com/r/") {
        return reddit::subreddit_from_url(input);
    }
    let trimmed = input.trim_start_matches('/');
    if let Some(rest) = trimmed.strip_prefix("r/") {
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        return (!name.is_empty()).then_some(name);
    }
    None
}

/// Build a YouTube per-channel RSS candidate from a handle/id/URL (resolving handles as needed).
async fn youtube_candidate(
    client: &Client,
    cfg: &IngestSettings,
    input: &str,
) -> Result<Option<Candidate>> {
    let is_youtube = input.contains("youtube.com") || input.contains("youtu.be");
    let is_handle = input.starts_with('@');
    let is_channel_id = looks_like_channel_id(input);

    if !(is_youtube || is_handle || is_channel_id) {
        return Ok(None);
    }

    // Direct channel id (bare, or in a /channel/UC… or ?channel_id= URL).
    if let Some(id) = extract_channel_id_from_input(input) {
        return Ok(Some(youtube_from_id(client, cfg, &id).await));
    }

    // Resolve a handle / custom URL by fetching the channel page.
    let page_url = if is_handle {
        format!("https://www.youtube.com/{input}")
    } else {
        input.to_string()
    };
    let guarded = match url_util::guard_public_url(&page_url, cfg.allow_private) {
        Ok(u) => u,
        Err(_) => return Ok(None),
    };
    if let Some(body) = fetch_body(client, cfg, &guarded).await {
        let html = String::from_utf8_lossy(&body);
        if let Some(id) = extract_channel_id_from_html(&html) {
            return Ok(Some(youtube_from_id(client, cfg, &id).await));
        }
    }
    Ok(None)
}

/// Candidate for a resolved channel id. The channel RSS carries the channel name, so fetch it
/// once here - otherwise the feed is created title-less and the UI shows the raw feed URL (in
/// the picker and in Manage) until the first scheduled poll fills the title in.
async fn youtube_from_id(client: &Client, cfg: &IngestSettings, id: &str) -> Candidate {
    let feed_url = format!("https://www.youtube.com/feeds/videos.xml?channel_id={id}");
    let site_url = format!("https://www.youtube.com/channel/{id}");
    if let Some(body) = fetch_body(client, cfg, &feed_url).await {
        if let Some(c) = youtube_candidate_from_body(&body, &feed_url, site_url.clone()) {
            return c;
        }
    }
    // Discovery stays best-effort: an unreachable/unparseable RSS still yields the candidate.
    Candidate {
        feed_url,
        title: None,
        kind: FeedKind::Youtube.as_str().to_string(),
        site_url: Some(site_url),
        icon_url: None,
    }
}

/// Parse a fetched channel-RSS body into a titled YouTube candidate (`None` if it isn't a feed).
fn youtube_candidate_from_body(body: &[u8], feed_url: &str, site_url: String) -> Option<Candidate> {
    feed_kind_of(body)?;
    let mut c = feed_candidate(body, feed_url, FeedKind::Youtube);
    c.site_url = c.site_url.or(Some(site_url));
    Some(c)
}

fn looks_like_channel_id(s: &str) -> bool {
    s.len() == 24
        && s.starts_with("UC")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn extract_channel_id_from_input(input: &str) -> Option<String> {
    if looks_like_channel_id(input) {
        return Some(input.to_string());
    }
    for marker in ["/channel/", "channel_id="] {
        if let Some(idx) = input.find(marker) {
            let rest = &input[idx + marker.len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if looks_like_channel_id(&id) {
                return Some(id);
            }
        }
    }
    None
}

fn extract_channel_id_from_html(html: &str) -> Option<String> {
    for marker in ["\"channelId\":\"", "\"externalId\":\"", "channel_id="] {
        if let Some(idx) = html.find(marker) {
            let rest = &html[idx + marker.len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if looks_like_channel_id(&id) {
                return Some(id);
            }
        }
    }
    None
}

/// GET a URL and return the body bytes, or `None` on any failure/not-modified (best effort).
async fn fetch_body(client: &Client, cfg: &IngestSettings, url: &str) -> Option<Vec<u8>> {
    match fetch::get(
        client,
        url,
        &Conditional {
            etag: None,
            last_modified: None,
        },
        cfg,
    )
    .await
    {
        Ok(FetchOutcome::Fetched(f)) => Some(f.body),
        _ => None,
    }
}

/// Return the parsed feed kind if the body is a feed, else `None`.
fn feed_kind_of(body: &[u8]) -> Option<FeedKind> {
    use feed_rs::model::FeedType;
    let feed = feed_rs::parser::parse(body).ok()?;
    Some(match feed.feed_type {
        FeedType::Atom => FeedKind::Atom,
        FeedType::JSON => FeedKind::JsonFeed,
        FeedType::RSS0 | FeedType::RSS1 | FeedType::RSS2 => FeedKind::Rss,
    })
}

fn feed_candidate(body: &[u8], feed_url: &str, kind: FeedKind) -> Candidate {
    let parsed = feed_rs::parser::parse(body).ok();
    let title = parsed
        .as_ref()
        .and_then(|f| f.title.as_ref().map(|t| t.content.trim().to_string()))
        .filter(|s| !s.is_empty());
    let site_url = parsed.as_ref().and_then(|f| {
        f.links
            .iter()
            .find(|l| l.rel.as_deref() == Some("alternate"))
            .map(|l| l.href.clone())
    });
    Candidate {
        feed_url: url_util::normalize_url(feed_url).unwrap_or_else(|| feed_url.to_string()),
        title,
        kind: kind.as_str().to_string(),
        site_url,
        icon_url: None,
    }
}

/// Extract `<link rel="alternate" type="application/rss+xml|atom+xml|json">` hrefs from HTML.
fn sniff_alternate_links(body: &[u8], base: &str) -> Vec<Candidate> {
    use scraper::{Html, Selector};
    let html = String::from_utf8_lossy(body);
    let doc = Html::parse_document(&html);
    let Ok(sel) = Selector::parse("link[rel~=alternate][href]") else {
        return vec![];
    };
    let mut out = Vec::new();
    for el in doc.select(&sel) {
        let ty = el.value().attr("type").unwrap_or("").to_ascii_lowercase();
        let kind = if ty.contains("atom") {
            FeedKind::Atom
        } else if ty.contains("json") {
            FeedKind::JsonFeed
        } else if ty.contains("rss") || ty.contains("xml") {
            FeedKind::Rss
        } else {
            continue;
        };
        if let Some(href) = el
            .value()
            .attr("href")
            .and_then(|h| url_util::resolve(base, h))
        {
            out.push(Candidate {
                feed_url: url_util::normalize_url(&href).unwrap_or(href),
                title: el.value().attr("title").map(|s| s.to_string()),
                kind: kind.as_str().to_string(),
                site_url: Some(base.to_string()),
                icon_url: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_reddit_forms() {
        assert_eq!(reddit_subreddit("r/rust"), Some("rust".into()));
        assert_eq!(reddit_subreddit("/r/rust/"), Some("rust".into()));
        assert_eq!(
            reddit_subreddit("https://www.reddit.com/r/programming/.rss"),
            Some("programming".into())
        );
        assert_eq!(reddit_subreddit("https://example.com"), None);
    }

    #[test]
    fn detects_youtube_channel_id() {
        let id = "UC_x5XG1OV2P6uZZ5FSM9Ttw"; // 24 chars
        assert!(looks_like_channel_id(id));
        assert_eq!(extract_channel_id_from_input(id).as_deref(), Some(id));
        assert_eq!(
            extract_channel_id_from_input(&format!("https://www.youtube.com/channel/{id}"))
                .as_deref(),
            Some(id)
        );
        assert_eq!(
            extract_channel_id_from_html(&format!("x\"channelId\":\"{id}\"y")).as_deref(),
            Some(id)
        );
    }

    #[test]
    fn youtube_channel_rss_body_yields_a_titled_candidate() {
        // Shape of https://www.youtube.com/feeds/videos.xml?channel_id=… (Atom + yt namespace).
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns="http://www.w3.org/2005/Atom">
              <id>yt:channel:kVfrGwV-iG9bSsgCbrNPxQ</id>
              <yt:channelId>kVfrGwV-iG9bSsgCbrNPxQ</yt:channelId>
              <title>ThePrimeagen</title>
              <link rel="alternate" href="https://www.youtube.com/channel/UCkVfrGwV-iG9bSsgCbrNPxQ"/>
            </feed>"#;
        let feed_url = "https://www.youtube.com/feeds/videos.xml?channel_id=UCkVfrGwV-iG9bSsgCbrNPxQ";
        let c = youtube_candidate_from_body(
            xml,
            feed_url,
            "https://www.youtube.com/channel/UCkVfrGwV-iG9bSsgCbrNPxQ".to_string(),
        )
        .unwrap();
        assert_eq!(c.title.as_deref(), Some("ThePrimeagen"));
        assert_eq!(c.kind, "youtube");
        assert_eq!(c.feed_url, feed_url);
        assert_eq!(
            c.site_url.as_deref(),
            Some("https://www.youtube.com/channel/UCkVfrGwV-iG9bSsgCbrNPxQ")
        );
    }

    #[test]
    fn non_feed_body_yields_no_youtube_candidate() {
        assert!(youtube_candidate_from_body(
            b"<html>not a feed</html>",
            "https://www.youtube.com/feeds/videos.xml?channel_id=x",
            "https://www.youtube.com/channel/x".to_string(),
        )
        .is_none());
    }

    #[test]
    fn sniffs_alternate_links() {
        let html = br#"<html><head>
          <link rel="alternate" type="application/rss+xml" href="/feed.xml" title="RSS">
          <link rel="alternate" type="application/atom+xml" href="https://ex.com/atom">
        </head></html>"#;
        let cands = sniff_alternate_links(html, "https://ex.com/blog");
        assert_eq!(cands.len(), 2);
        assert!(cands
            .iter()
            .any(|c| c.feed_url == "https://ex.com/feed.xml"));
        assert!(cands.iter().any(|c| c.kind == "atom"));
    }
}
