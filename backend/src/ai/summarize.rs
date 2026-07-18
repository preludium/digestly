//! On-demand summarization (prompt.md §6, §6a). Checks the shared cache first, then calls the
//! configured text route and caches the result by item, provider, and summary kind. For video
//! items it lazily fetches the transcript and produces a structured
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
    pub summary_kind: String,
    pub model: String,
    pub cached: bool,
}

struct NewSummary<'a> {
    provider_id: i64,
    summary_kind: &'a str,
    model: &'a str,
    api_style: &'a str,
    text: &'a str,
    is_video: bool,
}

/// Why a summarize call couldn't produce a summary - each maps to a clear, key-free API error.
pub enum SummarizeError {
    /// No text provider configured (admin must add one).
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

/// Summarize item `item_id` with the configured text route. Caller must have already verified the user's
/// access to the item (per-user scoping). `force` regenerates even if a cached summary exists.
pub async fn summarize_item(
    pool: &SqlitePool,
    http: &Client,
    enc_key: &[u8; 32],
    item_id: i64,
    force: bool,
) -> Result<SummaryResult, SummarizeError> {
    let text_route = provider::load_text_route(pool, enc_key).await?;

    let mut item = load_item(pool, item_id).await?;
    let is_video = item.kind == "youtube";

    // Dedicated video provider (§6a): Gemini summarizes the video by URL - no transcript needed.
    let video_provider = if is_video {
        provider::load_video_provider(pool, enc_key).await?
    } else {
        None
    };

    // Cache lookup follows the same preference order as live calls. Provider identity and summary
    // kind prevent same-named models and native-video output from colliding.
    if !force {
        if let Some(vp) = &video_provider {
            if let Some(text) = cached(pool, item_id, vp.id, &vp.model, "video-topics-v1").await? {
                return Ok(SummaryResult {
                    summary: text,
                    summary_kind: "video-topics-v1".into(),
                    model: vp.model.clone(),
                    cached: true,
                });
            }
        }
        for text_provider in &text_route {
            if let Some(text) = cached(
                pool,
                item_id,
                text_provider.id,
                &text_provider.model,
                if is_video {
                    "text-video-topics-v1"
                } else {
                    "text"
                },
            )
            .await?
            {
                return Ok(SummaryResult {
                    summary: text,
                    summary_kind: if is_video {
                        "text-video-topics-v1"
                    } else {
                        "text"
                    }
                    .into(),
                    model: text_provider.model.clone(),
                    cached: true,
                });
            }
        }
    }

    if video_provider.is_none() && text_route.is_empty() {
        return Err(SummarizeError::NotConfigured);
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
                params.timeout_secs,
            )
            .await
            {
                Ok(resp) => {
                    budget::record(pool, resp.tokens_used).await;
                    store_summary(
                        pool,
                        item_id,
                        NewSummary {
                            provider_id: vp.id,
                            summary_kind: "video-topics-v1",
                            model: &vp.model,
                            api_style: vp.api_style.as_str(),
                            text: &resp.text,
                            is_video: true,
                        },
                    )
                    .await?;
                    return Ok(SummaryResult {
                        summary: resp.text,
                        summary_kind: "video-topics-v1".into(),
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

    let req = LlmRequest {
        system,
        user,
        max_tokens: params.max_tokens,
        temperature: params.temperature,
    };

    let mut last_error = None;
    for text_provider in &text_route {
        let llm = client::make_client(
            http.clone(),
            text_provider.api_style,
            text_provider.base_url.clone(),
            text_provider.model.clone(),
            text_provider.key.clone(),
            params.timeout_secs,
        );
        match llm.complete(&req).await {
            Ok(resp) => {
                budget::record(pool, resp.tokens_used).await;
                store_summary(
                    pool,
                    item_id,
                    NewSummary {
                        provider_id: text_provider.id,
                        summary_kind: if is_video {
                            "text-video-topics-v1"
                        } else {
                            "text"
                        },
                        model: &text_provider.model,
                        api_style: text_provider.api_style.as_str(),
                        text: &resp.text,
                        is_video,
                    },
                )
                .await?;
                return Ok(SummaryResult {
                    summary: resp.text,
                    summary_kind: if is_video {
                        "text-video-topics-v1"
                    } else {
                        "text"
                    }
                    .into(),
                    model: text_provider.model.clone(),
                    cached: false,
                });
            }
            Err(error) => {
                tracing::warn!(item_id, provider_id = text_provider.id, error = %error,
                    "text provider failed - trying the next configured provider");
                last_error = Some(error.user_message());
            }
        }
    }
    Err(last_error.map_or(SummarizeError::NotConfigured, SummarizeError::Provider))
}

/// Prompt for the video-URL path (§6a): Gemini watches the attached video itself and produces a
/// concise topic list. One text part - the native call has no system turn.
fn build_video_url_prompt(item: &ItemRow) -> String {
    format!(
        "Watch the attached YouTube video and output plain Markdown only: 3-6 bullets in exactly \
         '- **Topic:** explanation' form. Cover distinct aspects discussed in the video and explain \
         what the video says about each. Do not use timestamps, chronology, chapters, questions, \
         takeaways, a conclusion, audience targeting, or a watch recommendation. Do not include a \
         prelude, heading, or closing prose. Do not invent facts not in the video.\n\nTitle: {}\nSource: {}",
        item.title.as_deref().unwrap_or("(untitled)"),
        item.feed_title
    )
}

/// Build (system prompt, source text, was-truncated). Reading vs video/description-based (§6a).
fn build_prompt(item: &ItemRow, is_video: bool) -> (String, Option<String>, bool) {
    if is_video {
        let has_captions = item.transcript_status == "fetched";
        let (raw, system) = if has_captions {
            (
                item.transcript_text.clone(),
                "Turn this video's captions into plain Markdown only: 3-6 bullets in exactly '- **Topic:** \
                 explanation' form. Cover distinct aspects discussed in the video and explain what the \
                 video says about each. Do not use timestamps, chronology, chapters, questions, takeaways, \
                 a conclusion, audience targeting, or a watch recommendation. Do not include a prelude, \
                 heading, or closing prose. Do not invent facts not in the captions."
                    .to_string(),
            )
        } else {
            // No captions → summarize the description, labelled (prompt.md §6a).
            (
                text_of(item),
                "No fetched captions are available for this video, so you are given only its description. \
                  The first line must be exactly 'Description-only topics (no captions available):'. Then \
                  output 2-4 bullets in exactly '- **Topic:** explanation' form. Do not claim any video \
                  details beyond the description. Do not use timestamps, chronology, chapters, questions, \
                  takeaways, a conclusion, audience targeting, or a watch recommendation. Do not include a \
                  prelude or closing prose."
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
    provider_id: i64,
    model: &str,
    summary_kind: &str,
) -> Result<Option<String>, sqlx::Error> {
    Ok(sqlx::query(
        "SELECT summary_text FROM item_summaries
              WHERE item_id = ? AND provider_id = ? AND model = ? AND summary_kind = ?
                AND TRIM(summary_text) <> ''",
    )
    .bind(item_id)
    .bind(provider_id)
    .bind(model)
    .bind(summary_kind)
    .fetch_optional(pool)
    .await?
    .map(|r| r.get("summary_text")))
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
    summary: NewSummary<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO item_summaries (item_id, provider_id, summary_kind, model, api_style, summary_text)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(item_id, provider_id, summary_kind) DO UPDATE SET
              model        = excluded.model,
              summary_text = excluded.summary_text,
             api_style    = excluded.api_style,
             created_at   = datetime('now')",
    )
    .bind(item_id)
    .bind(summary.provider_id)
    .bind(summary.summary_kind)
    .bind(summary.model)
    .bind(summary.api_style)
    .bind(summary.text)
    .execute(pool)
    .await?;

    // Reading time: for video items the summary IS the readable content; for articles only fill a
    // missing value (don't clobber the article's own reading time).
    let secs = content::reading_time_secs(summary.text);
    if summary.is_video {
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
    fn video_url_prompt_requires_topic_bullets_without_legacy_sections() {
        let it = item("youtube", "none", None, None);
        let p = build_video_url_prompt(&it);
        for requirement in [
            "3-6 bullets",
            "- **Topic:** explanation",
            "timestamps, chronology, chapters, questions, takeaways",
            "conclusion, audience targeting, or a watch recommendation",
        ] {
            assert!(
                p.contains(requirement),
                "missing requirement: {requirement}"
            );
        }
        assert!(p.contains("Title: T"));
        assert!(p.contains("prelude, heading, or closing prose"));
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
        assert!(system.contains("- **Topic:**"));
        assert!(system.contains("timestamps, chronology, chapters"));
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
            system.contains("Description-only topics (no captions available):"),
            "preserves the visible description-only label"
        );
        assert!(system.contains("timestamps, chronology, chapters, questions"));
        assert!(system.contains("watch recommendation"));
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

    async fn set_text_route(pool: &SqlitePool, ids: &[i64]) {
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, 'ordered')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(provider::TEXT_PROVIDER_MODE_KEY)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(provider::TEXT_ROUTE_PROVIDER_IDS_KEY)
        .bind(serde_json::to_string(ids).unwrap())
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
        // Providers store the OpenAI-compatible base (`…/openai`); the native video call uses
        // Gemini's Interactions endpoint and documented URL-video input shape.
        let mock = spawn_mock(axum::Router::new().route(
            "/interactions",
            axum::routing::post(
                |headers: axum::http::HeaderMap,
                 axum::Json(body): axum::Json<serde_json::Value>| async move {
                    assert_eq!(headers["x-goog-api-key"], "k");
                    assert_eq!(body["model"], "gemini-vid");
                    assert!(body["input"][0]["text"].is_string());
                    assert_eq!(body["input"][0]["type"], "text");
                    assert_eq!(
                        body["input"][1],
                        serde_json::json!({
                            "type": "video",
                            "uri": "https://www.youtube.com/watch?v=abc123xyz00"
                        })
                    );
                    axum::Json(serde_json::json!({
                        "steps": [{
                            "type": "model_output",
                            "content": [{ "type": "text", "text": "Video summary from Gemini." }]
                        }],
                        "usage": { "total_tokens": 1000 }
                    }))
                },
            ),
        ))
        .await;
        // Active provider is unreachable: success proves the video path was used.
        let text_provider =
            make_provider(&pool, "groq", "http://127.0.0.1:9", "text-model", true).await;
        let vp = make_provider(
            &pool,
            "gemini",
            &format!("{mock}/openai"),
            "gemini-vid",
            false,
        )
        .await;
        set_video_provider(&pool, vp).await;
        let item_id = make_video_item(&pool, "none", None).await;
        // Both historical YouTube kinds are deliberately cache misses.
        for (provider_id, summary_kind, model) in [
            (vp, "video", "gemini-vid"),
            (text_provider, "text", "text-model"),
        ] {
            sqlx::query(
                "INSERT INTO item_summaries (item_id, provider_id, summary_kind, model, api_style, summary_text)
                 VALUES (?, ?, ?, ?, 'openai', 'legacy')",
            )
            .bind(item_id)
            .bind(provider_id)
            .bind(summary_kind)
            .bind(model)
            .execute(&pool)
            .await
            .unwrap();
        }

        let http = Client::new();
        let res = summarize_item(&pool, &http, &ENC_KEY, item_id, false)
            .await
            .map_err(|_| "summarize failed")
            .unwrap();
        assert_eq!(res.summary, "Video summary from Gemini.");
        assert_eq!(res.summary_kind, "video-topics-v1");
        assert_eq!(res.model, "gemini-vid");
        assert!(!res.cached);

        let kind: String = sqlx::query(
            "SELECT summary_kind FROM item_summaries WHERE item_id = ? AND provider_id = ?",
        )
        .bind(item_id)
        .bind(vp)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("summary_kind");
        assert_eq!(
            kind, "video",
            "legacy row remains alongside the v1 cache row"
        );

        // Force regenerates the selected v1 row without overwriting the legacy row.
        let forced = summarize_item(&pool, &http, &ENC_KEY, item_id, true)
            .await
            .map_err(|_| "forced summarize failed")
            .unwrap();
        assert!(!forced.cached);
        let kinds: Vec<String> = sqlx::query(
            "SELECT summary_kind FROM item_summaries WHERE item_id = ? AND provider_id = ? ORDER BY summary_kind",
        )
        .bind(item_id)
        .bind(vp)
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get("summary_kind"))
        .collect();
        assert_eq!(kinds, ["video", "video-topics-v1"]);

        // The transcript was never lazily fetched (no youtube.com scraping on this path).
        let row =
            sqlx::query("SELECT transcript_status, reading_time_secs FROM items WHERE id = ?")
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
        assert_eq!(res2.summary_kind, "video-topics-v1");
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
        let fallback = make_provider(&pool, "groq", &mock, "text-model", true).await;
        let failed_text =
            make_provider(&pool, "openai", "http://127.0.0.1:9", "failed", false).await;
        // Video provider points at a dead port → the video path fails fast and must fall back.
        let vp = make_provider(&pool, "gemini", "http://127.0.0.1:9", "gemini-vid", false).await;
        set_video_provider(&pool, vp).await;
        // Native video fails first. The text route also fails once before succeeding, proving the
        // fallback uses the complete ordered route rather than only the legacy active provider.
        set_text_route(&pool, &[failed_text, fallback]).await;
        // Transcript already fetched, so the fallback needs no youtube.com access either.
        let item_id = make_video_item(&pool, "fetched", Some("the transcript text")).await;

        let http = Client::new();
        let res = summarize_item(&pool, &http, &ENC_KEY, item_id, false)
            .await
            .map_err(|_| "summarize failed")
            .unwrap();
        assert_eq!(res.summary, "Fallback summary");
        assert_eq!(res.summary_kind, "text-video-topics-v1");
        assert_eq!(res.model, "text-model");
    }

    #[tokio::test]
    async fn cache_identity_distinguishes_provider_and_summary_kind() {
        let pool = test_pool().await;
        let item_id = make_video_item(&pool, "fetched", Some("the transcript text")).await;

        store_summary(
            &pool,
            item_id,
            NewSummary {
                provider_id: 10,
                summary_kind: "text",
                model: "same-model",
                api_style: "openai",
                text: "text",
                is_video: true,
            },
        )
        .await
        .unwrap();
        store_summary(
            &pool,
            item_id,
            NewSummary {
                provider_id: 11,
                summary_kind: "video",
                model: "same-model",
                api_style: "openai",
                text: "video",
                is_video: true,
            },
        )
        .await
        .unwrap();

        assert_eq!(
            cached(&pool, item_id, 10, "same-model", "text")
                .await
                .unwrap()
                .as_deref(),
            Some("text")
        );
        assert_eq!(
            cached(&pool, item_id, 11, "same-model", "video")
                .await
                .unwrap()
                .as_deref(),
            Some("video")
        );
        assert!(cached(&pool, item_id, 10, "same-model", "video")
            .await
            .unwrap()
            .is_none());
        assert!(cached(&pool, item_id, 11, "same-model", "text")
            .await
            .unwrap()
            .is_none());
        assert!(cached(&pool, item_id, 10, "changed-model", "text")
            .await
            .unwrap()
            .is_none());
        store_summary(
            &pool,
            item_id,
            NewSummary {
                provider_id: 12,
                summary_kind: "text-video-topics-v1",
                model: "same-model",
                api_style: "openai",
                text: "   ",
                is_video: true,
            },
        )
        .await
        .unwrap();
        assert!(
            cached(&pool, item_id, 12, "same-model", "text-video-topics-v1")
                .await
                .unwrap()
                .is_none()
        );
    }
}
