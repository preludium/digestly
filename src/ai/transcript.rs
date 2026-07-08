//! YouTube caption fetch for the "read, don't watch" feature (prompt.md §3, §6a).
//!
//! Best-effort: pull the watch page, find the `captionTracks` list in the embedded player
//! response, prefer a manual (non-ASR) track in the user's-ish language, fetch the timedtext XML,
//! and flatten it to plain text. Any failure/empty result → `Unavailable` (caller falls back to the
//! description, clearly labelled). Timeout + size caps throughout; runs lazily so it never blocks
//! other work.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Client;

use crate::ingest::settings::USER_AGENT;

/// Cap on the watch-page HTML we read while hunting for caption tracks.
const PAGE_CAP: usize = 4 * 1024 * 1024;
/// Cap on the transcript XML we read.
const XML_CAP: usize = 3 * 1024 * 1024;
/// Cap on stored transcript length (chars) — long transcripts are condensed by the summarizer.
const TEXT_CAP: usize = 100_000;

/// Outcome of a transcript fetch.
pub enum Transcript {
    /// Captions were found and flattened to text.
    Text(String),
    /// No usable captions (→ `transcript_status = 'unavailable'`).
    Unavailable,
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
        if let Some(v) = u.query_pairs().find(|(k, _)| k == "v").map(|(_, v)| v.to_string()) {
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
    !s.is_empty() && s.len() <= 20 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Fetch and flatten a video's captions. Never errors — returns `Unavailable` on any problem.
pub async fn fetch(client: &Client, video_id: &str, timeout_secs: u64) -> Transcript {
    match try_fetch(client, video_id, timeout_secs).await {
        Some(text) if !text.trim().is_empty() => Transcript::Text(cap(text, TEXT_CAP)),
        _ => Transcript::Unavailable,
    }
}

async fn try_fetch(client: &Client, video_id: &str, timeout_secs: u64) -> Option<String> {
    let watch = format!("https://www.youtube.com/watch?v={video_id}&hl=en");
    let page = get_capped(client, &watch, timeout_secs, PAGE_CAP).await?;
    let base_url = pick_track_url(&page)?;
    let xml = get_capped(client, &base_url, timeout_secs, XML_CAP).await?;
    Some(flatten_timedtext(&xml))
}

/// Locate the `captionTracks` array in the player response and choose the best track's baseUrl.
fn pick_track_url(page: &str) -> Option<String> {
    let idx = page.find("\"captionTracks\":")?;
    let after = &page[idx + "\"captionTracks\":".len()..];
    let array = balanced_array(after)?;
    let tracks: serde_json::Value = serde_json::from_str(array).ok()?;
    let tracks = tracks.as_array()?;
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
    best.get("baseUrl").and_then(|u| u.as_str()).map(str::to_string)
}

/// Take a balanced `[...]` array from the start of `s` (which begins at or before the `[`).
fn balanced_array(s: &str) -> Option<&str> {
    let start = s.find('[')?;
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Flatten timedtext XML (`<text ...>line</text>` nodes) to newline-joined plain text.
fn flatten_timedtext(xml: &str) -> String {
    let mut out = String::new();
    for chunk in xml.split("<text").skip(1) {
        let Some(gt) = chunk.find('>') else { continue };
        let rest = &chunk[gt + 1..];
        let Some(end) = rest.find("</text>") else { continue };
        let line = decode_entities(&rest[..end]);
        let line = line.trim();
        if !line.is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
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
            _ => entity
                .strip_prefix('#')
                .and_then(|n| {
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

async fn get_capped(client: &Client, url: &str, timeout_secs: u64, cap: usize) -> Option<String> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if buf.len() + chunk.len() > cap {
            break; // enough — the caption tracks appear early in the page
        }
        buf.extend_from_slice(&chunk);
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
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

    #[test]
    fn extracts_video_ids() {
        assert_eq!(video_id(Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ"), None).as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(video_id(Some("https://youtu.be/dQw4w9WgXcQ?t=5"), None).as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(video_id(Some("https://www.youtube.com/shorts/abc123_-XYZ"), None).as_deref(), Some("abc123_-XYZ"));
        assert_eq!(video_id(None, Some("yt:video:dQw4w9WgXcQ")).as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(video_id(Some("https://example.com/post"), None), None);
    }

    #[test]
    fn flattens_and_decodes() {
        let xml = r#"<transcript><text start="0" dur="1">Hello &amp; welcome</text><text start="1" dur="1">it&#39;s me</text></transcript>"#;
        assert_eq!(flatten_timedtext(xml), "Hello & welcome\nit's me\n");
    }

    #[test]
    fn picks_manual_english_track() {
        let page = r#"junk"captionTracks":[{"baseUrl":"http://asr","kind":"asr","languageCode":"en"},{"baseUrl":"http://manual","languageCode":"en"}]more"#;
        assert_eq!(pick_track_url(page).as_deref(), Some("http://manual"));
    }

    #[test]
    fn no_tracks_is_none() {
        assert_eq!(pick_track_url("no tracks here"), None);
        assert_eq!(pick_track_url(r#""captionTracks":[]"#), None);
    }
}
