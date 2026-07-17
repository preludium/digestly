//! AI provider management + global AI params (prompt.md §6, §9.7 AI tab, §10). **Every** endpoint
//! requires `role = admin` (via `AdminUser`) - enforced server-side, not just hidden in the UI
//! (§11). Keys are write-only: submitted on create, encrypted at rest, and NEVER returned/logged.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

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
    /// `single` uses the selected provider, falling back to the active provider by default;
    /// `ordered` tries configured providers in order.
    text_provider_mode: String,
    /// Effective text provider IDs, filtered for the dedicated video provider.
    text_provider_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct AiSettingsInput {
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    daily_token_budget: Option<i64>,
    #[serde(default)]
    monthly_token_budget: Option<i64>,
    #[serde(default)]
    text_provider_mode: Option<String>,
    #[serde(default)]
    text_provider_ids: Option<Vec<i64>>,
    /// Absent preserves this setting; `null` clears it.
    #[serde(default)]
    video_provider_id: SettingField<i64>,
}

#[derive(Default)]
enum SettingField<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

impl<'de, T> Deserialize<'de> for SettingField<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

async fn get_ai_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<AiSettingsDto>> {
    let p = ai::AiParams::load(&state.pool).await;
    let (day, month) = ai::budget::spent(&state.pool)
        .await
        .map_err(AppError::Internal)?;
    let video_provider_id = provider::selected_video_provider_id(&state.pool)
        .await
        .map_err(AppError::Internal)?;
    let text_provider_mode = provider::text_provider_mode(&state.pool)
        .await
        .map_err(AppError::Internal)?;
    let text_provider_ids = provider::selected_text_provider_ids(&state.pool)
        .await
        .map_err(AppError::Internal)?;
    Ok(Json(AiSettingsDto {
        max_tokens: p.max_tokens,
        temperature: p.temperature,
        timeout_secs: p.timeout_secs,
        daily_token_budget: p.daily_token_budget,
        monthly_token_budget: p.monthly_token_budget,
        tokens_used_today: day,
        tokens_used_month: month,
        video_provider_id,
        text_provider_mode,
        text_provider_ids,
    }))
}

async fn put_ai_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<AiSettingsInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut tx = state.pool.begin().await?;
    let current_video_id = provider::selected_video_provider_id_tx(&mut tx)
        .await
        .map_err(AppError::Internal)?;
    let video_provider_is_present = !matches!(body.video_provider_id, SettingField::Absent);
    let video_provider_id = match body.video_provider_id {
        SettingField::Absent => current_video_id,
        SettingField::Null => None,
        SettingField::Value(id) => Some(id),
    };

    if let Some(mode) = body.text_provider_mode.as_deref() {
        if !matches!(mode, "single" | "ordered") {
            return Err(AppError::BadRequest(
                "text_provider_mode must be 'single' or 'ordered'".into(),
            ));
        }
    }
    let selected_text_ids = if let Some(ids) = body.text_provider_ids.as_ref() {
        let mut unique_ids = ids.clone();
        unique_ids.sort_unstable();
        unique_ids.dedup();
        if unique_ids.len() != ids.len() {
            return Err(AppError::BadRequest(
                "text provider IDs must be unique".into(),
            ));
        }
        if ids.iter().any(|id| Some(*id) == video_provider_id) {
            return Err(AppError::BadRequest(
                "text provider IDs cannot include the video provider".into(),
            ));
        }
        for id in ids {
            if provider::load_one_tx(&mut tx, &state.enc_key, *id)
                .await
                .map_err(AppError::Internal)?
                .is_none()
            {
                return Err(AppError::NotFound("text provider not found".into()));
            }
        }
        Some(ids.clone())
    } else {
        None
    };

    if let Some(id) = video_provider_id {
        ensure_gemini_provider_tx(&mut tx, &state.enc_key, id).await?;
    }

    let temperature = body
        .temperature
        .map(|value| (value.clamp(0.0, 2.0) * 100.0).round() as i64);
    let max_tokens = body.max_tokens.map(|value| value.clamp(64, 8192));
    let timeout_secs = body.timeout_secs.map(|value| value.clamp(5, 300));
    let daily_token_budget = body.daily_token_budget.map(|value| value.max(0));
    let monthly_token_budget = body.monthly_token_budget.map(|value| value.max(0));
    let text_provider_ids_json = selected_text_ids
        .map(|ids| serde_json::to_string(&ids).map_err(|error| AppError::Internal(error.into())))
        .transpose()?;

    if let Some(value) = max_tokens {
        set_setting_tx(&mut tx, "ai.max_tokens", &value.to_string()).await?;
    }
    if let Some(value) = temperature {
        set_setting_tx(&mut tx, "ai.temperature_x100", &value.to_string()).await?;
    }
    if let Some(value) = timeout_secs {
        set_setting_tx(&mut tx, "ai.timeout_secs", &value.to_string()).await?;
    }
    if let Some(value) = daily_token_budget {
        set_setting_tx(&mut tx, "ai.daily_token_budget", &value.to_string()).await?;
    }
    if let Some(value) = monthly_token_budget {
        set_setting_tx(&mut tx, "ai.monthly_token_budget", &value.to_string()).await?;
    }
    if let Some(mode) = body.text_provider_mode {
        set_setting_tx(&mut tx, provider::TEXT_PROVIDER_MODE_KEY, &mode).await?;
    }
    if let Some(ids) = text_provider_ids_json {
        set_setting_tx(&mut tx, provider::TEXT_ROUTE_PROVIDER_IDS_KEY, &ids).await?;
    }
    if video_provider_is_present {
        match video_provider_id {
            Some(id) => {
                set_setting_tx(&mut tx, provider::VIDEO_PROVIDER_KEY, &id.to_string()).await?
            }
            None => {
                sqlx::query("DELETE FROM app_settings WHERE key = ?")
                    .bind(provider::VIDEO_PROVIDER_KEY)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    tx.commit().await?;
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
async fn put_video_provider(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<VideoProviderInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut tx = state.pool.begin().await?;
    match body.provider_id {
        None => {
            sqlx::query("DELETE FROM app_settings WHERE key = ?")
                .bind(provider::VIDEO_PROVIDER_KEY)
                .execute(&mut *tx)
                .await?;
        }
        Some(id) => {
            ensure_gemini_provider_tx(&mut tx, &state.enc_key, id).await?;
            set_setting_tx(&mut tx, provider::VIDEO_PROVIDER_KEY, &id.to_string()).await?;
        }
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn set_setting_tx(tx: &mut Transaction<'_, Sqlite>, key: &str, value: &str) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn ensure_gemini_provider_tx(
    tx: &mut Transaction<'_, Sqlite>,
    enc_key: &[u8; 32],
    id: i64,
) -> ApiResult<()> {
    let provider = provider::load_one_tx(tx, enc_key, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("provider not found".into()))?;
    if provider.provider_type != "gemini" {
        return Err(AppError::BadRequest(
            "the video provider must be a Gemini provider".into(),
        ));
    }
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
