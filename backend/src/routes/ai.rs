//! AI provider management + global AI params (prompt.md §6, §9.7 AI tab, §10). **Every** endpoint
//! requires `role = admin` (via `AdminUser`) - enforced server-side, not just hidden in the UI
//! (§11). Keys are write-only: submitted on create, encrypted at rest, and NEVER returned/logged.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::ai::{self, client, provider, ApiStyle, LlmRequest};
use crate::auth::extract::AdminUser;
use crate::error::{ApiResult, AppError};
use crate::http::AppState;
use crate::ingest::url_util;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/ai/presets", get(get_presets))
        .route("/ai/providers", get(list_providers).post(create_provider))
        .route(
            "/ai/providers/:id",
            axum::routing::patch(patch_provider).delete(delete_provider),
        )
        .route("/ai/providers/:id/activate", post(activate_provider))
        .route("/ai/providers/:id/test", post(test_provider))
        .route("/ai/settings", get(get_ai_settings).put(put_ai_settings))
        .route("/ai/video-provider", axum::routing::put(put_video_provider))
}

// ---------------------------------------------------------------------------
// Presets & list
// ---------------------------------------------------------------------------

async fn get_presets(_admin: AdminUser) -> Json<Vec<ai::Preset>> {
    Json(ai::presets())
}

async fn list_providers(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<provider::ProviderInfo>>> {
    Ok(Json(
        provider::list(&state.pool)
            .await
            .map_err(AppError::Internal)?,
    ))
}

// ---------------------------------------------------------------------------
// Create / patch / activate / delete
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateProvider {
    name: String,
    provider_type: String,
    api_style: String,
    base_url: String,
    model: String,
    /// Write-only: encrypted at rest, never returned.
    key: Option<String>,
}

async fn create_provider(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateProvider>,
) -> ApiResult<Json<serde_json::Value>> {
    let name = body.name.trim();
    let base_url = body.base_url.trim();
    let model = body.model.trim();
    if name.is_empty() || base_url.is_empty() || model.is_empty() {
        return Err(AppError::BadRequest(
            "name, base_url and model are required".into(),
        ));
    }
    let api_style = ApiStyle::parse(&body.api_style)
        .ok_or_else(|| AppError::BadRequest("api_style must be 'openai' or 'anthropic'".into()))?;

    // SSRF guard on custom base URLs (prompt.md §6, §11). Ollama is intentionally allowed to use
    // localhost/LAN; otherwise honor the admin `allow-private` toggle.
    let is_ollama = body.provider_type.eq_ignore_ascii_case("ollama");
    let allow_private = is_ollama || allow_private(&state.pool).await;
    url_util::guard_public_url(base_url, allow_private)
        .map_err(|e| AppError::BadRequest(format!("invalid base URL: {e}")))?;

    let id = provider::create(
        &state.pool,
        &state.enc_key,
        provider::NewProvider {
            name: name.to_string(),
            provider_type: body.provider_type.trim().to_string(),
            api_style,
            base_url: base_url.to_string(),
            model: model.to_string(),
            key: body.key,
        },
    )
    .await
    .map_err(AppError::Internal)?;

    Ok(Json(serde_json::json!({ "id": id })))
}

#[derive(Deserialize)]
struct PatchProvider {
    name: Option<String>,
    model: Option<String>,
}

async fn patch_provider(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchProvider>,
) -> ApiResult<Json<serde_json::Value>> {
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let model = body
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let ok = provider::patch(&state.pool, id, name, model)
        .await
        .map_err(AppError::Internal)?;
    if !ok {
        return Err(AppError::NotFound("provider not found".into()));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn activate_provider(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let ok = provider::activate(&state.pool, id)
        .await
        .map_err(AppError::Internal)?;
    if !ok {
        return Err(AppError::NotFound("provider not found".into()));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_provider(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let ok = provider::delete(&state.pool, id)
        .await
        .map_err(AppError::Internal)?;
    if !ok {
        return Err(AppError::NotFound("provider not found".into()));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Test connection
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct TestResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// `POST /api/ai/providers/{id}/test` - a tiny live call, reports ok/error. Never echoes the key.
async fn test_provider(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<TestResult>> {
    let resolved = provider::load_one(&state.pool, &state.enc_key, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("provider not found".into()))?;

    let params = ai::AiParams::load(&state.pool).await;
    let llm = client::make_client(
        state.http_client.clone(),
        resolved.api_style,
        resolved.base_url,
        resolved.model,
        resolved.key,
        params.timeout_secs.min(30),
    );
    let req = LlmRequest {
        system: "You are a connectivity test. Reply with exactly: OK".to_string(),
        user: "Reply with exactly: OK".to_string(),
        max_tokens: 16,
        temperature: 0.0,
    };

    match llm.complete(&req).await {
        Ok(_) => Ok(Json(TestResult {
            ok: true,
            error: None,
        })),
        Err(e) => Ok(Json(TestResult {
            ok: false,
            error: Some(e.user_message()),
        })),
    }
}

// ---------------------------------------------------------------------------
// Global AI params
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AiSettingsDto {
    max_tokens: u32,
    temperature: f32,
    timeout_secs: u64,
    daily_token_budget: i64,
    monthly_token_budget: i64,
    tokens_used_today: i64,
    tokens_used_month: i64,
    /// Dedicated Gemini provider for YouTube items (prompt.md §6a video path); null = off.
    video_provider_id: Option<i64>,
}

#[derive(Deserialize)]
struct AiSettingsInput {
    max_tokens: u32,
    temperature: f32,
    timeout_secs: u64,
    daily_token_budget: i64,
    monthly_token_budget: i64,
}

async fn get_ai_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<AiSettingsDto>> {
    let p = ai::AiParams::load(&state.pool).await;
    let (day, month) = ai::budget::spent(&state.pool)
        .await
        .map_err(AppError::Internal)?;
    let video_provider_id = sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(provider::VIDEO_PROVIDER_KEY)
        .fetch_optional(&state.pool)
        .await?
        .and_then(|r| r.get::<String, _>("value").parse::<i64>().ok());
    Ok(Json(AiSettingsDto {
        max_tokens: p.max_tokens,
        temperature: p.temperature,
        timeout_secs: p.timeout_secs,
        daily_token_budget: p.daily_token_budget,
        monthly_token_budget: p.monthly_token_budget,
        tokens_used_today: day,
        tokens_used_month: month,
        video_provider_id,
    }))
}

async fn put_ai_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<AiSettingsInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let temp_x100 = (body.temperature.clamp(0.0, 2.0) * 100.0).round() as i64;
    set_setting(
        &state.pool,
        "ai.max_tokens",
        &body.max_tokens.clamp(64, 8192).to_string(),
    )
    .await?;
    set_setting(&state.pool, "ai.temperature_x100", &temp_x100.to_string()).await?;
    set_setting(
        &state.pool,
        "ai.timeout_secs",
        &body.timeout_secs.clamp(5, 300).to_string(),
    )
    .await?;
    set_setting(
        &state.pool,
        "ai.daily_token_budget",
        &body.daily_token_budget.max(0).to_string(),
    )
    .await?;
    set_setting(
        &state.pool,
        "ai.monthly_token_budget",
        &body.monthly_token_budget.max(0).to_string(),
    )
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Video provider (prompt.md §6a video path)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct VideoProviderInput {
    /// `null` clears the slot (video items go back to the transcript flow).
    provider_id: Option<i64>,
}

/// `PUT /api/ai/video-provider` - point the video slot at a Gemini provider, or clear it.
/// Gemini-only: it's the only supported API that accepts a YouTube URL as model input.
async fn put_video_provider(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<VideoProviderInput>,
) -> ApiResult<Json<serde_json::Value>> {
    match body.provider_id {
        None => {
            sqlx::query("DELETE FROM app_settings WHERE key = ?")
                .bind(provider::VIDEO_PROVIDER_KEY)
                .execute(&state.pool)
                .await?;
        }
        Some(id) => {
            let p = provider::load_one(&state.pool, &state.enc_key, id)
                .await
                .map_err(AppError::Internal)?
                .ok_or_else(|| AppError::NotFound("provider not found".into()))?;
            if p.provider_type != "gemini" {
                return Err(AppError::BadRequest(
                    "the video provider must be a Gemini provider - it's the only API that can \
                     summarize a YouTube video by URL"
                        .into(),
                ));
            }
            set_setting(&state.pool, provider::VIDEO_PROVIDER_KEY, &id.to_string()).await?;
        }
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// The shared SSRF allow-private toggle (`ingest.allow_private`, prompt.md §8/§11).
async fn allow_private(pool: &SqlitePool) -> bool {
    sqlx::query("SELECT value FROM app_settings WHERE key = 'ingest.allow_private'")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|r| {
            let v: String = r.get("value");
            v == "true" || v == "1"
        })
        .unwrap_or(false)
}
