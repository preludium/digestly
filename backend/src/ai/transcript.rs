//! YouTube caption fetch for the "read, don't watch" feature (prompt.md §3, §6a).
//!
//! Best-effort: pull the watch page just to lift the public `INNERTUBE_API_KEY`, then ask
//! YouTube's internal `youtubei/v1/player` endpoint for the player response *as the Android app
//! client* - captions embedded directly in the watch page's HTML now require a proof-of-origin
//! token and come back empty on a plain fetch, but the Android-client player response's caption
//! URLs don't have that gate. Prefer a manual (non-ASR) track in the user's-ish language, strip
//! `&fmt=srv3` so the timedtext response is the flat legacy XML (not the nested per-word srv3
//! dialect), fetch it, and flatten to plain text. Any permanent failure/empty result →
//! `Unavailable` (caller falls back to the description, clearly labelled) - but a `429` on any of
//! these requests is transient, not "no captions", so it's surfaced distinctly (`RateLimited`)
//! and the caller leaves `transcript_status = 'none'` instead of writing the item off forever.
//! Timeout + size caps throughout; runs lazily so it never blocks other work.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Client;
use sqlx::SqlitePool;
use tracing::info;

use crate::ingest::settings::USER_AGENT;

/// Cap on the watch-page HTML we read while hunting for caption tracks.
const PAGE_CAP: usize = 4 * 1024 * 1024;
/// Cap on the transcript XML we read.
const XML_CAP: usize = 3 * 1024 * 1024;
/// Cap on stored transcript length (chars) - long transcripts are condensed by the summarizer.
const TEXT_CAP: usize = 100_000;

/// Outcome of a transcript fetch.
enum Transcript {
    /// Captions were found and flattened to text.
    Text(String),
    /// No usable captions (→ `transcript_status = 'unavailable'`).
    Unavailable,
    /// This is (or was) a livestream - `videoDetails.isLiveContent` on the player response. The
    /// background worker drops these from the library entirely; the on-demand path treats them
    /// like any other caption-less video.
    LiveContent,
    /// YouTube rate-limited one of the requests (HTTP 429) - transient, not "no captions". The
    /// caller should leave `transcript_status` as `'none'` so this gets retried later instead of
    /// being permanently written off.
    RateLimited,
}

/// Extract a YouTube video id from an item URL or GUID (prompt.md §3).
/// Handles `watch?v=`, `youtu.be/`, `/embed/`, `/shorts/`, and `yt:video:<id>` guids.
pub fn video_id(url: Option<&str>, guid: Option<&str>) -> Option<String> {
    if let Some(g) = guid {
        if let Some(id) = g.strip_prefix("yt:video:") {
            if is_id(id) {
                return Some(id.to_string());
            }
        }
    }
    let url = url?;
    if let Ok(u) = url::Url::parse(url) {
        // watch?v=<id>
        if let Some(v) = u
            .query_pairs()
            .find(|(k, _)| k == "v")
            .map(|(_, v)| v.to_string())
        {
            if is_id(&v) {
                return Some(v);
            }
        }
        let host = u.host_str().unwrap_or("");
        let segs: Vec<&str> = u.path().split('/').filter(|s| !s.is_empty()).collect();
        if host.contains("youtu.be") {
            if let Some(id) = segs.first() {
                if is_id(id) {
                    return Some((*id).to_string());
                }
            }
        }
        for (i, seg) in segs.iter().enumerate() {
            if (*seg == "embed" || *seg == "shorts" || *seg == "v") && i + 1 < segs.len() {
                let id = segs[i + 1];
                if is_id(id) {
                    return Some(id.to_string());
                }
            }
        }
    }
    None
}

fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 20
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Fetch and flatten a video's captions. Never errors.
async fn fetch(client: &Client, video_id: &str, timeout_secs: u64) -> Transcript {
    let watch = format!("https://www.youtube.com/watch?v={video_id}&hl=en");
    let page = match get_capped(client, &watch, timeout_secs, PAGE_CAP).await {
        Ok(p) => p,
        Err(FetchIssue::RateLimited) => return Transcript::RateLimited,
        Err(FetchIssue::Failed) => return Transcript::Unavailable,
    };
    let Some(api_key) = extract_innertube_api_key(&page) else {
        return Transcript::Unavailable;
    };
    let body = match post_innertube_player(client, &api_key, video_id, timeout_secs, PAGE_CAP).await
    {
        Ok(b) => b,
        Err(FetchIssue::RateLimited) => return Transcript::RateLimited,
        Err(FetchIssue::Failed) => return Transcript::Unavailable,
    };
    let Ok(player) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Transcript::Unavailable;
    };
    if is_live_content(&player) {
        return Transcript::LiveContent;
    }
    let Some(base_url) = pick_caption_track(&player) else {
        return Transcript::Unavailable;
    };
    let xml = match get_capped(client, &base_url, timeout_secs, XML_CAP).await {
        Ok(x) => x,
        Err(FetchIssue::RateLimited) => return Transcript::RateLimited,
        Err(FetchIssue::Failed) => return Transcript::Unavailable,
    };
    let text = flatten_timedtext(&xml);
    if text.trim().is_empty() {
        Transcript::Unavailable
    } else {
        Transcript::Text(cap(text, TEXT_CAP))
    }
}

/// True if the player response marks this video as live/was-live (`videoDetails.isLiveContent`).
/// Missing → `false` (falls through to normal caption handling rather than mis-classifying a
/// video whose player data we simply couldn't read as live).
fn is_live_content(player: &serde_json::Value) -> bool {
    player
        .get("videoDetails")
        .and_then(|d| d.get("isLiveContent"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Fetch a video's transcript and persist it (best-effort; always sets `transcript_status` to
/// `fetched` or `unavailable`). Used by the on-demand summarize path - never deletes the item
/// (a user actively viewing it shouldn't have it vanish out from under them), so live content
/// just falls back to `unavailable` like any other no-captions video. The background transcript
/// worker uses [`fetch_store_or_remove_if_live`] instead, which excludes live recordings outright.
pub async fn fetch_and_store(
    pool: &SqlitePool,
    http: &Client,
    item_id: i64,
    url: Option<&str>,
    guid: Option<&str>,
    timeout_secs: u64,
) {
    let Some(vid) = video_id(url, guid) else {
        let _ = set_transcript(pool, item_id, None, "unavailable").await;
        return;
    };
    match fetch(http, &vid, timeout_secs).await {
        Transcript::Text(text) => {
            let _ = set_transcript(pool, item_id, Some(&text), "fetched").await;
        }
        // Live content is just another caption-less video here: the user is actively looking at
        // this item, so it must not vanish - only the background worker removes live recordings.
        Transcript::Unavailable | Transcript::LiveContent => {
            let _ = set_transcript(pool, item_id, None, "unavailable").await;
        }
        // Transient - leave transcript_status as 'none' so a later summarize retries it.
        Transcript::RateLimited => {}
    }
}

/// What happened to one item in [`fetch_store_or_remove_if_live`] - lets the worker log/tally a
/// batch summary instead of the caller having to guess.
pub enum TranscriptOutcome {
    Fetched,
    Unavailable,
    RemovedLive,
    /// Transient YouTube rate-limit - left as `transcript_status = 'none'`, retried next batch.
    RateLimited,
}

/// Fetch a video's transcript for the background worker: persists it on success, persists
/// `unavailable` on a plain no-captions video, and **deletes the item entirely** on live content
/// (livestreams/recordings are excluded from the library, not just left without a transcript -
/// prompt.md §3 "just regular videos", not reels/Shorts/live).
pub async fn fetch_store_or_remove_if_live(
    pool: &SqlitePool,
    http: &Client,
    item_id: i64,
    url: Option<&str>,
    guid: Option<&str>,
    timeout_secs: u64,
) -> TranscriptOutcome {
    let Some(vid) = video_id(url, guid) else {
        info!(item_id, "no resolvable video id - marking unavailable");
        let _ = set_transcript(pool, item_id, None, "unavailable").await;
        return TranscriptOutcome::Unavailable;
    };
    match fetch(http, &vid, timeout_secs).await {
        Transcript::Text(text) => {
            info!(item_id, video_id = %vid, chars = text.len(), "transcript fetched");
            let _ = set_transcript(pool, item_id, Some(&text), "fetched").await;
            TranscriptOutcome::Fetched
        }
        Transcript::Unavailable => {
            info!(item_id, video_id = %vid, "no captions available - marking unavailable");
            let _ = set_transcript(pool, item_id, None, "unavailable").await;
            TranscriptOutcome::Unavailable
        }
        Transcript::LiveContent => {
            info!(item_id, video_id = %vid, "live recording - removing from library");
            let _ = sqlx::query("DELETE FROM items WHERE id = ?")
                .bind(item_id)
                .execute(pool)
                .await;
            TranscriptOutcome::RemovedLive
        }
        Transcript::RateLimited => {
            info!(item_id, video_id = %vid, "rate-limited - will retry next batch");
            TranscriptOutcome::RateLimited
        }
    }
}

async fn set_transcript(
    pool: &SqlitePool,
    item_id: i64,
    text: Option<&str>,
    status: &str,
) -> Result<(), sqlx::Error> {
    // A fetched transcript IS the video's readable content (prompt.md §3 "read, don't watch"),
    // so refresh reading time from it - the ingest-time value only measured the description.
    // COALESCE keeps that estimate when there's no transcript (status 'unavailable'). A later
    // summary still overwrites this for videos (`summarize::store_summary`).
    let reading_secs = text.map(crate::ingest::content::reading_time_secs);
    sqlx::query(
        "UPDATE items SET transcript_text = ?, transcript_status = ?,
                reading_time_secs = COALESCE(?, reading_time_secs)
         WHERE id = ?",
    )
    .bind(text)
    .bind(status)
    .bind(reading_secs)
    .bind(item_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Pull the public `INNERTUBE_API_KEY` out of the watch page. Not hardcoded - re-extracted each
/// time so a key rotation on YouTube's side doesn't quietly break every fetch.
fn extract_innertube_api_key(html: &str) -> Option<String> {
    const MARKER: &str = "\"INNERTUBE_API_KEY\":\"";
    let idx = html.find(MARKER)?;
    let after = &html[idx + MARKER.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// Choose the best caption track's baseUrl from a `youtubei/v1/player` response body, with
/// `&fmt=srv3` stripped so the URL serves the flat `<text start dur>` XML `flatten_timedtext`
/// expects instead of the nested per-word srv3 format.
fn pick_caption_track(player: &serde_json::Value) -> Option<String> {
    let tracks = player
        .get("captions")?
        .get("playerCaptionsTracklistRenderer")?
        .get("captionTracks")?
        .as_array()?;
    if tracks.is_empty() {
        return None;
    }

    // Prefer manual (kind != "asr") English, then any manual, then English ASR, then anything.
    let score = |t: &serde_json::Value| -> i32 {
        let asr = t.get("kind").and_then(|k| k.as_str()) == Some("asr");
        let en = t
            .get("languageCode")
            .and_then(|l| l.as_str())
            .map(|l| l.starts_with("en"))
            .unwrap_or(false);
        match (asr, en) {
            (false, true) => 0,
            (false, false) => 1,
            (true, true) => 2,
            (true, false) => 3,
        }
    };
    let best = tracks.iter().min_by_key(|t| score(t))?;
    let base_url = best.get("baseUrl").and_then(|u| u.as_str())?;
    Some(base_url.replace("&fmt=srv3", ""))
}

/// Flatten timedtext XML (`<text ...>line</text>` nodes) to readable flowing text: cues joined
/// with spaces (a cue boundary is a meaningless mid-sentence break for a reader), entities fully
/// decoded, paragraph breaks inserted (see [`readable_transcript`]).
fn flatten_timedtext(xml: &str) -> String {
    let mut out = String::new();
    for chunk in xml.split("<text").skip(1) {
        let Some(gt) = chunk.find('>') else { continue };
        let rest = &chunk[gt + 1..];
        let Some(end) = rest.find("</text>") else {
            continue;
        };
        let line = rest[..end].trim();
        if !line.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(line);
        }
    }
    readable_transcript(&out)
}

/// Turn raw caption text into a readable transcript: collapse the cue-per-line shape into
/// flowing sentences, decode entities until stable (YouTube double-encodes - `&amp;#39;` needs
/// two passes), and insert paragraph breaks at sentence ends. Idempotent enough to also reflow
/// transcripts stored before this existed (see `maintenance::reflow_transcripts_once`).
pub fn readable_transcript(raw: &str) -> String {
    let joined = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    paragraphize(&decode_entities_fully(&joined))
}

/// Soft paragraph target: once past this many chars, break at the next sentence end.
const PARA_TARGET_CHARS: usize = 500;
/// Hard ceiling for auto-captions with no punctuation at all.
const PARA_HARD_CHARS: usize = 900;

/// Insert blank-line paragraph breaks into flowing text at sentence boundaries.
fn paragraphize(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 64);
    let mut para_len = 0usize;
    for word in text.split_whitespace() {
        if para_len > 0 {
            out.push(' ');
            para_len += 1;
        }
        out.push_str(word);
        para_len += word.chars().count();
        let ends_sentence = matches!(word.chars().last(), Some('.' | '!' | '?'));
        if (para_len >= PARA_TARGET_CHARS && ends_sentence) || para_len >= PARA_HARD_CHARS {
            out.push_str("\n\n");
            para_len = 0;
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Decode entities until the text stops changing (bounded): YouTube's timedtext bodies are
/// double-encoded, so a single pass leaves literal `&#39;` in the stored transcript.
fn decode_entities_fully(s: &str) -> String {
    let mut current = s.to_string();
    for _ in 0..3 {
        let next = decode_entities(&current);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// Decode the small set of XML/HTML entities YouTube emits (incl. numeric refs).
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        let rest = &s[i..];
        let Some(semi) = rest.find(';').filter(|&p| p <= 10) else {
            out.push('&');
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity.strip_prefix('#').and_then(|n| {
                let code = if let Some(hex) = n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    n.parse::<u32>().ok()
                };
                code.and_then(char::from_u32)
            }),
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                // Advance past the entity (consume up to and including ';').
                for _ in 0..semi {
                    chars.next();
                }
            }
            None => out.push('&'),
        }
    }
    out
}

/// A request-level failure, distinguishing a transient rate-limit (429) from anything else so
/// callers can decide whether to retry later instead of giving up permanently.
enum FetchIssue {
    RateLimited,
    Failed,
}

async fn get_capped(client: &Client, url: &str, timeout_secs: u64, cap: usize) -> Result<String, FetchIssue> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|_| FetchIssue::Failed)?;
    read_capped(resp, cap).await
}

/// Ask YouTube's internal player endpoint for the player response *as the Android app client*
/// (`context.client.clientName: ANDROID`) - its caption URLs aren't gated behind the
/// proof-of-origin token that the plain watch page's embedded URLs now require.
async fn post_innertube_player(
    client: &Client,
    api_key: &str,
    video_id: &str,
    timeout_secs: u64,
    cap: usize,
) -> Result<String, FetchIssue> {
    let url = format!("https://www.youtube.com/youtubei/v1/player?key={api_key}");
    let body = serde_json::json!({
        "context": {"client": {"clientName": "ANDROID", "clientVersion": "20.10.38"}},
        "videoId": video_id,
    });
    let resp = client
        .post(&url)
        .timeout(Duration::from_secs(timeout_secs))
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .json(&body)
        .send()
        .await
        .map_err(|_| FetchIssue::Failed)?;
    read_capped(resp, cap).await
}

async fn read_capped(resp: reqwest::Response, cap: usize) -> Result<String, FetchIssue> {
    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(FetchIssue::RateLimited);
    }
    if !resp.status().is_success() {
        return Err(FetchIssue::Failed);
    }
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| FetchIssue::Failed)?;
        if buf.len() + chunk.len() > cap {
            break; // enough - the caption tracks appear early in the response
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn cap(mut s: String, max: usize) -> String {
    if s.chars().count() > max {
        s = s.chars().take(max).collect();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use sqlx::Row;

    /// Item with a description-based reading time, awaiting its transcript.
    async fn make_video_item(pool: &SqlitePool, reading_time_secs: i64) -> i64 {
        let feed_id: i64 = sqlx::query(
            "INSERT INTO feeds (feed_url, kind, fetch_interval_secs) VALUES ('https://example.com/yt', 'youtube', 3600) RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap()
        .get("id");
        sqlx::query(
            "INSERT INTO items (feed_id, dedup_hash, transcript_status, reading_time_secs, published_at)
             VALUES (?, 'v1', 'none', ?, datetime('now')) RETURNING id",
        )
        .bind(feed_id)
        .bind(reading_time_secs)
        .fetch_one(pool)
        .await
        .unwrap()
        .get("id")
    }

    async fn reading_time(pool: &SqlitePool, item_id: i64) -> i64 {
        sqlx::query("SELECT reading_time_secs FROM items WHERE id = ?")
            .bind(item_id)
            .fetch_one(pool)
            .await
            .unwrap()
            .get("reading_time_secs")
    }

    #[tokio::test]
    async fn storing_a_fetched_transcript_refreshes_reading_time() {
        let pool = test_pool().await;
        // Description-based estimate: the 30s floor → "1 min read".
        let item = make_video_item(&pool, 30).await;

        // 400-word transcript ≈ 120s at 200 wpm.
        let transcript = "word ".repeat(400);
        set_transcript(&pool, item, Some(&transcript), "fetched")
            .await
            .unwrap();

        assert_eq!(reading_time(&pool, item).await, 120);
    }

    #[tokio::test]
    async fn unavailable_transcript_keeps_the_existing_reading_time() {
        let pool = test_pool().await;
        let item = make_video_item(&pool, 45).await;

        set_transcript(&pool, item, None, "unavailable")
            .await
            .unwrap();

        assert_eq!(reading_time(&pool, item).await, 45);
    }

    /// The player response as `fetch` parses it, so the tests exercise the same input shape.
    fn player(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn detects_live_content_from_video_details() {
        assert!(is_live_content(&player(
            r#"{"videoDetails":{"videoId":"x","isLiveContent":true}}"#
        )));
        assert!(!is_live_content(&player(
            r#"{"videoDetails":{"videoId":"x","isLiveContent":false}}"#
        )));
        // Missing → assume not live (falls through to normal caption handling).
        assert!(!is_live_content(&player(
            r#"{"playabilityStatus":{"status":"OK"}}"#
        )));
    }

    #[test]
    fn extracts_video_ids() {
        assert_eq!(
            video_id(Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ"), None).as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(
            video_id(Some("https://youtu.be/dQw4w9WgXcQ?t=5"), None).as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(
            video_id(Some("https://www.youtube.com/shorts/abc123_-XYZ"), None).as_deref(),
            Some("abc123_-XYZ")
        );
        assert_eq!(
            video_id(None, Some("yt:video:dQw4w9WgXcQ")).as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(video_id(Some("https://example.com/post"), None), None);
    }

    #[test]
    fn flattens_to_flowing_text_and_decodes() {
        // Cues are joined with spaces (a cue boundary is a meaningless mid-sentence break for a
        // reader), not newlines.
        let xml = r#"<transcript><text start="0" dur="1">Hello &amp; welcome</text><text start="1" dur="1">it&#39;s me</text></transcript>"#;
        assert_eq!(flatten_timedtext(xml), "Hello & welcome it's me");
    }

    #[test]
    fn decodes_youtubes_double_encoded_entities() {
        // Real timedtext bodies carry &amp;#39; - one decode pass leaves a literal &#39; that
        // then shows up verbatim in the UI.
        let xml = r#"<transcript><text start="0" dur="1">everyone&amp;#39;s asking</text></transcript>"#;
        assert_eq!(flatten_timedtext(xml), "everyone's asking");
    }

    #[test]
    fn long_transcripts_get_paragraph_breaks_at_sentence_ends() {
        let sentence = "This sentence talks about something interesting for quite a while today. ";
        let text = sentence.repeat(20); // ~1400 chars, plenty of sentence boundaries
        let readable = readable_transcript(&text);
        let paras: Vec<&str> = readable.split("\n\n").collect();
        assert!(paras.len() >= 2, "expected paragraph breaks, got: {readable}");
        for p in &paras {
            assert!(!p.contains('\n'), "no stray single newlines inside a paragraph");
            assert!(p.ends_with('.'), "paragraphs break at sentence ends: {p}");
        }
    }

    #[test]
    fn unpunctuated_asr_text_still_gets_hard_paragraph_breaks() {
        let text = "word ".repeat(600); // no sentence ends at all
        let readable = readable_transcript(&text);
        assert!(
            readable.contains("\n\n"),
            "hard break must kick in without punctuation"
        );
    }

    #[test]
    fn readable_transcript_reflows_previously_stored_cue_lines() {
        // Shape of transcripts stored before the reflow: one short cue per line, half-decoded.
        let stored = "GPT 5.6 Soul just came out and right now\neveryone&#39;s asking the same thing.";
        assert_eq!(
            readable_transcript(stored),
            "GPT 5.6 Soul just came out and right now everyone's asking the same thing."
        );
    }

    #[test]
    fn extracts_innertube_api_key() {
        let html = r#"junk stuff ,"INNERTUBE_API_KEY":"AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8","INNERTUBE_CONTEXT_CLIENT_NAME":1 more"#;
        assert_eq!(
            extract_innertube_api_key(html).as_deref(),
            Some("AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8")
        );
    }

    #[test]
    fn missing_api_key_is_none() {
        assert_eq!(extract_innertube_api_key("no key in this page"), None);
    }

    #[test]
    fn picks_manual_english_track_from_innertube_json() {
        let body = player(
            r#"{"captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[
            {"baseUrl":"http://asr","kind":"asr","languageCode":"en"},
            {"baseUrl":"http://manual","languageCode":"en"}
        ]}}}"#,
        );
        assert_eq!(pick_caption_track(&body).as_deref(), Some("http://manual"));
    }

    #[test]
    fn strips_srv3_format_param_from_chosen_track() {
        let body = player(
            r#"{"captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[
            {"baseUrl":"http://timedtext?v=x&fmt=srv3&lang=pl","kind":"asr","languageCode":"pl"}
        ]}}}"#,
        );
        assert_eq!(
            pick_caption_track(&body).as_deref(),
            Some("http://timedtext?v=x&lang=pl")
        );
    }

    #[test]
    fn no_captions_available_is_none() {
        // No captions block at all (e.g. captions disabled on the video).
        assert_eq!(
            pick_caption_track(&player(r#"{"playabilityStatus":{"status":"OK"}}"#)),
            None
        );
        // Empty captionTracks array (e.g. a past livestream with no auto-captions).
        assert_eq!(
            pick_caption_track(&player(
                r#"{"captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[]}}}"#
            )),
            None
        );
    }
}
