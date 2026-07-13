//! Reddit JSON source (prompt.md §3, §11). The `.rss` feed lacks score/comments, so we hit the
//! JSON endpoint for `score`/`num_comments`/`upvote_ratio` and read `selftext`. The public JSON
//! endpoint is unauthenticated and Reddit rate-limits/blocks it aggressively; when a Reddit
//! account is connected instance-wide (`oauth::reddit_polling_token`) the scheduler prefers the
//! authenticated `oauth.reddit.com` endpoint instead, which isn't subject to that blocking. Only
//! when neither works do we fall back to `.rss` (with NULL metrics) - the caller logs the bypass;
//! never silent.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::settings::{IngestSettings, USER_AGENT};
use super::{content, dedup_key, ParsedItem};

/// Extract the subreddit name from any reddit URL (`…/r/<sub>/…`).
pub fn subreddit_from_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let idx = lower.find("/r/")?;
    let rest = &url[idx + 3..];
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// The JSON listing URL used for metrics (top of the week, matches the seed cadence).
pub fn json_url(subreddit: &str) -> String {
    format!("https://www.reddit.com/r/{subreddit}/top.json?t=week&limit=50")
}

/// The authenticated equivalent of `json_url`, hit with a Bearer token via `fetch_authenticated`.
pub fn oauth_url(subreddit: &str) -> String {
    format!("https://oauth.reddit.com/r/{subreddit}/top?t=week&limit=50&raw_json=1")
}

/// The `.rss` fallback URL.
pub fn rss_url(subreddit: &str) -> String {
    format!("https://www.reddit.com/r/{subreddit}/.rss")
}

/// Fetch a subreddit listing via Reddit's authenticated API using a caller-supplied access
/// token (from `oauth::reddit_polling_token`). Not subject to the public JSON endpoint's
/// blocking, so score/comment data stays reliable. Returns the raw listing body for
/// `parse_listing`.
pub async fn fetch_authenticated(
    http: &reqwest::Client,
    subreddit: &str,
    access_token: &str,
) -> Result<Vec<u8>> {
    let res = http
        .get(oauth_url(subreddit))
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .context("reddit OAuth request failed")?;
    if !res.status().is_success() {
        anyhow::bail!("reddit OAuth endpoint returned {}", res.status());
    }
    Ok(res
        .bytes()
        .await
        .context("reddit OAuth body read failed")?
        .to_vec())
}

/// Parse a Reddit JSON listing into items with score/comments/upvote_ratio populated.
pub fn parse_listing(
    bytes: &[u8],
    cfg: &IngestSettings,
    fetched_at: DateTime<Utc>,
) -> Result<Vec<ParsedItem>> {
    let root: Value = serde_json::from_slice(bytes).context("reddit JSON parse failed")?;
    let children = root
        .get("data")
        .and_then(|d| d.get("children"))
        .and_then(|c| c.as_array())
        .context("reddit JSON missing data.children")?;

    let items = children
        .iter()
        .filter_map(|child| child.get("data"))
        .map(|d| parse_post(d, cfg, fetched_at))
        .collect();
    Ok(items)
}

fn parse_post(d: &Value, cfg: &IngestSettings, fetched_at: DateTime<Utc>) -> ParsedItem {
    let str_of = |k: &str| d.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let id = str_of("id");
    let title = str_of("title");
    let author = str_of("author").map(|a| format!("u/{a}"));
    let permalink = str_of("permalink").map(|p| format!("https://www.reddit.com{p}"));
    let external_url = str_of("url");
    let is_self = d.get("is_self").and_then(|v| v.as_bool()).unwrap_or(false);
    let selftext = str_of("selftext").unwrap_or_default();

    let score = d.get("score").and_then(|v| v.as_i64());
    let comments_count = d.get("num_comments").and_then(|v| v.as_i64());
    let upvote_ratio = d.get("upvote_ratio").and_then(|v| v.as_f64());

    let created = d.get("created_utc").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
    let published_at = DateTime::<Utc>::from_timestamp(created, 0).unwrap_or(fetched_at);

    // Build readable HTML: selftext for text posts, a link for link posts.
    let raw_html = if is_self && !selftext.is_empty() {
        selftext
            .split("\n\n")
            .filter(|p| !p.trim().is_empty())
            .map(|p| format!("<p>{}</p>", html_escape(p)))
            .collect::<String>()
    } else if let Some(ext) = external_url.clone().filter(|u| u.starts_with("http")) {
        format!(
            "<p><a href=\"{}\">{}</a></p>",
            html_escape(&ext),
            html_escape(&ext)
        )
    } else {
        String::new()
    };
    let base = permalink.as_deref().unwrap_or("https://www.reddit.com");
    let content_html = content::sanitize_html(&raw_html, Some(base), cfg.item_content_cap);
    let content_text = content::to_text(&content_html, cfg.item_content_cap);

    let thumbnail = str_of("thumbnail").filter(|t| t.starts_with("http"));
    let guid = id.as_deref().map(|i| format!("t3_{i}"));
    let dedup_hash = dedup_key(
        guid.as_deref(),
        permalink.as_deref(),
        title.as_deref(),
        Some(&content_text),
    );
    let reading_time_secs = Some(content::reading_time_secs(&content_text));

    ParsedItem {
        guid,
        url: permalink,
        title,
        author,
        content_html: Some(content_html).filter(|s| !s.is_empty()),
        content_text: Some(content_text).filter(|s| !s.is_empty()),
        image_url: thumbnail,
        duration_secs: None,
        reading_time_secs,
        published_at,
        score,
        comments_count,
        upvote_ratio,
        dedup_hash,
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_url_hits_the_authenticated_host_with_raw_json() {
        let url = oauth_url("rust");
        assert_eq!(
            url,
            "https://oauth.reddit.com/r/rust/top?t=week&limit=50&raw_json=1"
        );
    }

    #[test]
    fn extracts_subreddit_name() {
        assert_eq!(
            subreddit_from_url("https://www.reddit.com/r/programming/.rss"),
            Some("programming".into())
        );
        assert_eq!(
            subreddit_from_url("https://reddit.com/r/MachineLearning/top.json?t=week"),
            Some("MachineLearning".into())
        );
        assert_eq!(subreddit_from_url("https://example.com/feed"), None);
    }

    #[test]
    fn parses_score_and_comments() {
        let json = r#"{"data":{"children":[
            {"data":{"id":"abc","title":"Hello","author":"bob","permalink":"/r/x/comments/abc/hello/",
                     "is_self":true,"selftext":"body text here","score":123,"num_comments":45,
                     "upvote_ratio":0.98,"created_utc":1718010000.0,"thumbnail":"self"}}
        ]}}"#;
        let cfg = IngestSettings::default();
        let now = Utc::now();
        let items = parse_listing(json.as_bytes(), &cfg, now).unwrap();
        assert_eq!(items.len(), 1);
        let it = &items[0];
        assert_eq!(it.score, Some(123));
        assert_eq!(it.comments_count, Some(45));
        assert_eq!(it.upvote_ratio, Some(0.98));
        assert_eq!(it.title.as_deref(), Some("Hello"));
        assert_eq!(it.author.as_deref(), Some("u/bob"));
        assert!(it.content_text.as_ref().unwrap().contains("body text"));
        assert_eq!(it.image_url, None); // "self" isn't an http thumbnail
    }
}
