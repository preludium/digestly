//! On-demand summarization (prompt.md §6, §6a). Checks the shared cache first (any user's entry
//! for the active model counts), else calls the active provider and caches the result keyed by
//! (item, model). For video items it lazily fetches the transcript and produces a structured
//! readable summary; with no captions it summarizes the description, clearly labelled.

use anyhow::Result;
use reqwest::Client;
use sqlx::{Row, SqlitePool};

use super::{budget, client, provider, transcript, AiParams, LlmRequest};
use crate::ingest::content;

/// Cap on source text sent to the model (chars). Transcripts get a larger budget.
const READING_CAP: usize = 16_000;
const TRANSCRIPT_CAP: usize = 28_000;

/// A summarize result surfaced to the API.
pub struct SummaryResult {
    pub summary: String,
    pub model: String,
    pub cached: bool,
}

/// Why a summarize call couldn't produce a summary - each maps to a clear, key-free API error.
pub enum SummarizeError {
    /// No active provider configured (admin must add one).
    NotConfigured,
    /// The item has no text/transcript/description to summarize.
    NoContent,
    /// Daily/monthly token budget exhausted.
    Budget(String),
    /// The provider call failed (network/timeout/non-2xx).
    Provider(String),
    /// A database/internal error.
    Internal(anyhow::Error),
}

impl From<sqlx::Error> for SummarizeError {
    fn from(e: sqlx::Error) -> Self {
        SummarizeError::Internal(e.into())
    }
}
impl From<anyhow::Error> for SummarizeError {
    fn from(e: anyhow::Error) -> Self {
        SummarizeError::Internal(e)
    }
}

struct ItemRow {
    title: Option<String>,
    content_text: Option<String>,
    content_html: Option<String>,
    transcript_text: Option<String>,
    transcript_status: String,
    url: Option<String>,
    guid: Option<String>,
    kind: String,
    feed_title: String,
}

/// Summarize item `item_id` with the active provider. Caller must have already verified the user's
/// access to the item (per-user scoping). `force` regenerates even if a cached summary exists.
pub async fn summarize_item(
    pool: &SqlitePool,
    http: &Client,
    enc_key: &[u8; 32],
    item_id: i64,
    force: bool,
) -> Result<SummaryResult, SummarizeError> {
    let active = provider::load_active(pool, enc_key)
        .await?
        .ok_or(SummarizeError::NotConfigured)?;
    let model = active.model.clone();

    let mut item = load_item(pool, item_id).await?;
    let is_video = item.kind == "youtube";

    // Dedicated video provider (§6a): Gemini summarizes the video by URL - no transcript needed.
    let video_provider = if is_video {
        provider::load_video_provider(pool, enc_key).await?
    } else {
        None
    };

    // Cache hit (shared, keyed by item+model) - unless forced. For video items with a video
    // provider, that provider's model is the canonical cache slot; the active model's entry is
    // still honored second (it holds summaries made before the slot was set, or by fallback).
    if !force {
        if let Some(vp) = &video_provider {
            if let Some(text) = cached(pool, item_id, &vp.model).await? {
                return Ok(SummaryResult {
                    summary: text,
                    model: vp.model.clone(),
                    cached: true,
                });
            }
        }
        if let Some(text) = cached(pool, item_id, &model).await? {
            return Ok(SummaryResult {
                summary: text,
                model,
                cached: true,
            });
        }
    }

    let params = AiParams::load(pool).await;

    if let Some(vp) = &video_provider {
        if let Some(url) = item.url.clone() {
            // Budget guard before spending; video usage is large (~100 tokens/sec of video).
            budget::check(pool, &params)
                .await
                .map_err(SummarizeError::Budget)?;
            let prompt = build_video_url_prompt(&item);
            match client::gemini_video_complete(
                http.clone(),
                vp,
                &url,
                &prompt,
                video_max_tokens(params.max_tokens),
                params.temperature,
                params.timeout_secs,
            )
            .await
            {
                Ok(resp) => {
                    budget::record(pool, resp.tokens_used).await;
                    store_summary(
                        pool,
                        item_id,
                        &vp.model,
                        vp.api_style.as_str(),
                        &resp.text,
                        true,
                        force,
                    )
                    .await?;
                    return Ok(SummaryResult {
                        summary: resp.text,
                        model: vp.model.clone(),
                        cached: false,
                    });
                }
                // Private video, free-tier daily cap, rate limit, outage: never a dead end -
                // the transcript flow below is exactly what ran before this feature existed.
                Err(e) => tracing::warn!(item_id, error = %e,
                    "video provider failed - falling back to the transcript flow"),
            }
        }
    }

    // Video items: lazily fetch the transcript if we haven't tried yet (prompt.md §6a).
    if is_video && item.transcript_status == "none" {
        transcript::fetch_and_store(
            pool,
            http,
            item_id,
            item.url.as_deref(),
            item.guid.as_deref(),
            params.timeout_secs,
        )
        .await;
        item = load_item(pool, item_id).await?; // reload updated transcript fields
    }
    let (system, source, truncated) = build_prompt(&item, is_video);
    let source = match source {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Err(SummarizeError::NoContent),
    };

    // Budget guard (before spending tokens).
    budget::check(pool, &params)
        .await
        .map_err(SummarizeError::Budget)?;

    let mut user = format!(
        "Title: {}\nSource: {}\n\n{}:\n{}",
        item.title.as_deref().unwrap_or("(untitled)"),
        item.feed_title,
        if is_video { "Transcript" } else { "Article" },
        source
    );
    if truncated {
        user.push_str(
            "\n\n[Note: the source was truncated for length; summarize what is present.]",
        );
    }

    let llm = client::make_client(
        http.clone(),
        active.api_style,
        active.base_url,
        active.model,
        active.key,
        params.timeout_secs,
    );
    let req = LlmRequest {
        system,
        user,
        max_tokens: params.max_tokens,
        temperature: params.temperature,
    };

    let resp = llm
        .complete(&req)
        .await
        .map_err(|e| SummarizeError::Provider(e.user_message()))?;

    budget::record(pool, resp.tokens_used).await;
    store_summary(
        pool,
        item_id,
        &model,
        active.api_style.as_str(),
        &resp.text,
        is_video,
        force,
    )
    .await?;

    Ok(SummaryResult {
        summary: resp.text,
        model,
        cached: false,
    })
}

/// Prompt for the video-URL path (§6a): Gemini watches the attached video itself and produces a
/// comprehensive four-section brief (detailed breakdown, takeaways, answered questions, and a
/// watch-it pitch) so the reader can decide whether the video is worth their time. One text part
/// - the native call has no system turn.
fn build_video_url_prompt(item: &ItemRow) -> String {
    format!(
        "Act as an expert content analyst and researcher. The reader is deciding whether to \
         watch the attached YouTube video and needs a detailed breakdown to see if it's worth \
         their time. Watch the video and produce a comprehensive brief in plain text with these \
         four sections:\n\
         \n\
         1. Detailed chronological breakdown - a structured, step-by-step summary of what is \
         actually said, broken down by major topics or chapters, detailing the main arguments, \
         concepts, or stories the speaker shares in each section. Do not just list topics; \
         explain what the speaker says about them.\n\
         \n\
         2. Key takeaways & core insights - the top 3–5 most valuable, actionable, or profound \
         lessons a viewer should walk away with, each as a '- ' bullet.\n\
         \n\
         3. Questions this video answers - 5–7 specific, practical questions a viewer will get \
         answered by watching (e.g. \"How do I fix X error?\" or \"Why does Y happen?\"), each \
         as a '- ' bullet.\n\
         \n\
         4. The pitch: why watch it - a compelling argument for watching the video instead of \
         just reading this brief. Highlight unique elements: the speaker's energy or passion, \
         visual demonstrations, data charts, or code walkthroughs that are crucial to see, and \
         who the ideal audience is.\n\
         \n\
         Do not invent facts not in the video. Output the brief with no preamble, greeting, or \
         framing sentence (never anything like \"Here is a summary of…\") - the very first line \
         must be the section 1 heading.\n\nTitle: {}\nSource: {}",
        item.title.as_deref().unwrap_or("(untitled)"),
        item.feed_title
    )
}

/// Output-token floor for the video-URL path. Gemini's current flash models think by default and
/// thinking tokens count against `maxOutputTokens`, so the global text-summary default (1024)
/// leaves almost nothing for the visible brief - it came back truncated mid-sentence. This is
/// enough for the reasoning plus the four-section brief; an admin setting above it is respected.
const VIDEO_MIN_MAX_TOKENS: u32 = 4096;

fn video_max_tokens(configured: u32) -> u32 {
    configured.max(VIDEO_MIN_MAX_TOKENS)
}

/// Build (system prompt, source text, was-truncated). Reading vs video/description-based (§6a).
fn build_prompt(item: &ItemRow, is_video: bool) -> (String, Option<String>, bool) {
    if is_video {
        let has_captions = item.transcript_status == "fetched";
        let (raw, system) = if has_captions {
            (
                item.transcript_text.clone(),
                "You turn video transcripts into a readable article so the reader can read instead of \
                 watch. Output plain text with three parts: a 1–2 sentence intro; then 4–6 key-point \
                 bullets each starting with '- '; then a final line starting 'Takeaways: ' with the \
                 main conclusions. Aim for a 1–3 minute read. Do not invent facts not in the transcript."
                    .to_string(),
            )
        } else {
            // No captions → summarize the description, labelled (prompt.md §6a).
            (
                text_of(item),
                "No captions are available for this video, so you are given only its description. \
                 Summarize what the video is likely about BASED ON THE DESCRIPTION ONLY. Begin with \
                 'Description-based summary (no transcript available):' then 2–4 '- ' bullets. Do not \
                 fabricate details that aren't in the description."
                    .to_string(),
            )
        };
        let (src, trunc) = cap_opt(raw, TRANSCRIPT_CAP);
        (system, src, trunc)
    } else {
        let system =
            "You summarize articles for a busy reader. Output plain text: a one-sentence overview; \
             then 3–5 '- ' bullets of the key facts; then a final line starting 'Takeaway: '. Be \
             concise and do not invent facts."
                .to_string();
        let (src, trunc) = cap_opt(text_of(item), READING_CAP);
        (system, src, trunc)
    }
}

/// Prefer stored plain text; fall back to stripping sanitized HTML (prompt.md §5).
fn text_of(item: &ItemRow) -> Option<String> {
    if let Some(t) = item
        .content_text
        .as_deref()
        .filter(|t| !t.trim().is_empty())
    {
        return Some(t.to_string());
    }
    item.content_html
        .as_deref()
        .filter(|h| !h.trim().is_empty())
        .map(|h| content::to_text(h, READING_CAP * 2))
}

fn cap_opt(s: Option<String>, cap: usize) -> (Option<String>, bool) {
    match s {
        Some(s) if s.chars().count() > cap => (Some(s.chars().take(cap).collect()), true),
        other => (other, false),
    }
}

async fn cached(
    pool: &SqlitePool,
    item_id: i64,
    model: &str,
) -> Result<Option<String>, sqlx::Error> {
    Ok(
        sqlx::query("SELECT summary_text FROM item_summaries WHERE item_id = ? AND model = ?")
            .bind(item_id)
            .bind(model)
            .fetch_optional(pool)
            .await?
            .map(|r| r.get("summary_text")),
    )
}

async fn load_item(pool: &SqlitePool, item_id: i64) -> Result<ItemRow, SummarizeError> {
    let r = sqlx::query(
        "SELECT i.title, i.content_text, i.content_html, i.transcript_text, i.transcript_status,
                i.url, i.guid, fe.kind AS kind,
                COALESCE(NULLIF(fe.title, ''), fe.feed_url) AS feed_title
         FROM items i JOIN feeds fe ON fe.id = i.feed_id WHERE i.id = ?",
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| SummarizeError::Internal(anyhow::anyhow!("item vanished")))?;
    Ok(ItemRow {
        title: r.get("title"),
        content_text: r.get("content_text"),
        content_html: r.get("content_html"),
        transcript_text: r.get("transcript_text"),
        transcript_status: r.get("transcript_status"),
        url: r.get("url"),
        guid: r.get("guid"),
        kind: r.get("kind"),
        feed_title: r.get("feed_title"),
    })
}

/// Upsert the shared summary cache and refresh reading time (prompt.md §6, §6a).
async fn store_summary(
    pool: &SqlitePool,
    item_id: i64,
    model: &str,
    api_style: &str,
    summary: &str,
    is_video: bool,
    force: bool,
) -> Result<(), sqlx::Error> {
    // On force, refresh the existing row's text + timestamp; otherwise insert (or leave a race
    // winner in place).
    let _ = force;
    sqlx::query(
        "INSERT INTO item_summaries (item_id, model, api_style, summary_text)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(item_id, model) DO UPDATE SET
             summary_text = excluded.summary_text,
             api_style    = excluded.api_style,
             created_at   = datetime('now')",
    )
    .bind(item_id)
    .bind(model)
    .bind(api_style)
    .bind(summary)
    .execute(pool)
    .await?;

    // Reading time: for video items the summary IS the readable content; for articles only fill a
    // missing value (don't clobber the article's own reading time).
    let secs = content::reading_time_secs(summary);
    if is_video {
        sqlx::query("UPDATE items SET reading_time_secs = ? WHERE id = ?")
            .bind(secs)
            .bind(item_id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query(
            "UPDATE items SET reading_time_secs = COALESCE(reading_time_secs, ?) WHERE id = ?",
        )
        .bind(secs)
        .bind(item_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    fn item(
        kind: &str,
        transcript_status: &str,
        transcript: Option<&str>,
        text: Option<&str>,
    ) -> ItemRow {
        ItemRow {
            title: Some("T".into()),
            content_text: text.map(String::from),
            content_html: None,
            transcript_text: transcript.map(String::from),
            transcript_status: transcript_status.into(),
            url: Some("https://www.youtube.com/watch?v=abc".into()),
            guid: None,
            kind: kind.into(),
            feed_title: "F".into(),
        }
    }

    #[test]
    fn video_url_prompt_asks_for_the_four_section_brief() {
        let it = item("youtube", "none", None, None);
        let p = build_video_url_prompt(&it);
        for section in [
            "chronological breakdown",
            "Key takeaways",
            "Questions this video answers",
            "why watch it",
        ] {
            assert!(p.contains(section), "missing section: {section}");
        }
        assert!(p.contains("Title: T"));
        // No conversational preamble ("Here is a summary of…") - the brief must start at section 1.
        assert!(p.contains("no preamble"));
    }

    #[test]
    fn video_output_budget_never_drops_below_the_brief_floor() {
        // Thinking models spend maxOutputTokens on reasoning first; the comprehensive brief
        // needs headroom beyond the global text-summary default (1024).
        assert_eq!(video_max_tokens(1024), 4096);
        assert_eq!(video_max_tokens(8192), 8192);
    }

    #[test]
    fn reading_prompt_uses_article_text() {
        let (system, source, _) =
            build_prompt(&item("rss", "none", None, Some("article body")), false);
        assert!(system.contains("summarize articles"));
        assert_eq!(source.as_deref(), Some("article body"));
    }

    #[test]
    fn video_with_captions_uses_transcript() {
        let it = item("youtube", "fetched", Some("the transcript"), Some("desc"));
        let (system, source, _) = build_prompt(&it, true);
        assert!(system.contains("video transcripts"));
        assert_eq!(
            source.as_deref(),
            Some("the transcript"),
            "prefers transcript over description"
        );
    }

    #[test]
    fn video_without_captions_falls_back_to_description_labelled() {
        let it = item("youtube", "unavailable", None, Some("just the description"));
        let (system, source, _) = build_prompt(&it, true);
        assert!(
            system.contains("No captions are available"),
            "description-based, labelled (§6a)"
        );
        assert_eq!(source.as_deref(), Some("just the description"));
    }

    // -----------------------------------------------------------------------
    // Video-provider path (§6a): summarize by video URL via native Gemini, with
    // fallback to the transcript flow. Uses a local mock server - no real network.
    // -----------------------------------------------------------------------

    const ENC_KEY: [u8; 32] = [9u8; 32];

    async fn spawn_mock(router: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    async fn make_provider(
        pool: &SqlitePool,
        provider_type: &str,
        base_url: &str,
        model: &str,
        active: bool,
    ) -> i64 {
        let id = provider::create(
            pool,
            &ENC_KEY,
            provider::NewProvider {
                name: provider_type.to_string(),
                provider_type: provider_type.to_string(),
                api_style: crate::ai::ApiStyle::OpenAi,
                base_url: base_url.to_string(),
                model: model.to_string(),
                key: Some("k".to_string()),
            },
        )
        .await
        .unwrap();
        if active {
            provider::activate(pool, id).await.unwrap();
        }
        id
    }

    async fn set_video_provider(pool: &SqlitePool, id: i64) {
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(provider::VIDEO_PROVIDER_KEY)
        .bind(id.to_string())
        .execute(pool)
        .await
        .unwrap();
    }

    async fn make_video_item(
        pool: &SqlitePool,
        transcript_status: &str,
        transcript_text: Option<&str>,
    ) -> i64 {
        let feed_id: i64 = sqlx::query(
            "INSERT INTO feeds (feed_url, kind, fetch_interval_secs) VALUES ('https://example.com/yt', 'youtube', 3600) RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap()
        .get("id");
        sqlx::query(
            "INSERT INTO items (feed_id, dedup_hash, url, title, transcript_status, transcript_text, published_at)
             VALUES (?, 'v1', 'https://www.youtube.com/watch?v=abc123xyz00', 'T', ?, ?, datetime('now')) RETURNING id",
        )
        .bind(feed_id)
        .bind(transcript_status)
        .bind(transcript_text)
        .fetch_one(pool)
        .await
        .unwrap()
        .get("id")
    }

    #[tokio::test]
    async fn video_provider_summarizes_by_url_without_touching_transcripts() {
        let pool = test_pool().await;
        // Native Gemini mock: providers store the OpenAI-compat base (…/openai); the video call
        // must strip it and hit the native generateContent path.
        let mock = spawn_mock(axum::Router::new().route(
            "/models/gemini-vid:generateContent",
            axum::routing::post(|| async {
                axum::Json(serde_json::json!({
                    "candidates": [{ "content": { "parts": [{ "text": "Video summary from Gemini." }] } }],
                    "usageMetadata": { "totalTokenCount": 1000 }
                }))
            }),
        ))
        .await;
        // Active provider is unreachable: success proves the video path was used.
        make_provider(&pool, "groq", "http://127.0.0.1:9", "text-model", true).await;
        let vp = make_provider(&pool, "gemini", &format!("{mock}/openai"), "gemini-vid", false).await;
        set_video_provider(&pool, vp).await;
        let item_id = make_video_item(&pool, "none", None).await;

        let http = Client::new();
        let res = summarize_item(&pool, &http, &ENC_KEY, item_id, false)
            .await
            .map_err(|_| "summarize failed")
            .unwrap();
        assert_eq!(res.summary, "Video summary from Gemini.");
        assert_eq!(res.model, "gemini-vid");
        assert!(!res.cached);

        // The transcript was never lazily fetched (no youtube.com scraping on this path).
        let row = sqlx::query("SELECT transcript_status, reading_time_secs FROM items WHERE id = ?")
            .bind(item_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("transcript_status"), "none");
        // Reading time refreshed from the summary (video semantics).
        assert!(row.get::<i64, _>("reading_time_secs") > 0);

        // Second call hits the shared cache under the video provider's model.
        let res2 = summarize_item(&pool, &http, &ENC_KEY, item_id, false)
            .await
            .map_err(|_| "summarize failed")
            .unwrap();
        assert!(res2.cached);
        assert_eq!(res2.model, "gemini-vid");
    }

    #[tokio::test]
    async fn video_provider_failure_falls_back_to_the_transcript_flow() {
        let pool = test_pool().await;
        // OpenAI-compat mock for the ACTIVE provider (the fallback target).
        let mock = spawn_mock(axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|| async {
                axum::Json(serde_json::json!({
                    "choices": [{ "message": { "content": "Fallback summary" } }],
                    "usage": { "total_tokens": 42 }
                }))
            }),
        ))
        .await;
        make_provider(&pool, "groq", &mock, "text-model", true).await;
        // Video provider points at a dead port → the video path fails fast and must fall back.
        let vp = make_provider(&pool, "gemini", "http://127.0.0.1:9", "gemini-vid", false).await;
        set_video_provider(&pool, vp).await;
        // Transcript already fetched, so the fallback needs no youtube.com access either.
        let item_id = make_video_item(&pool, "fetched", Some("the transcript text")).await;

        let http = Client::new();
        let res = summarize_item(&pool, &http, &ENC_KEY, item_id, false)
            .await
            .map_err(|_| "summarize failed")
            .unwrap();
        assert_eq!(res.summary, "Fallback summary");
        assert_eq!(res.model, "text-model");
    }
}
