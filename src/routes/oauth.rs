//! OAuth import endpoints (prompt.md §3, §9.7 — Stretch S4). Per-user: link a YouTube/Reddit
//! account, then repeatedly **sync** subscribed channels/subreddits into your feeds (adding only
//! the ones you don't already have). Refresh tokens are stored encrypted and never returned.

use axum::extract::{Path, Query, State};
use axum::response::Redirect;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sqlx::Row;

use crate::auth::extract::CurrentUser;
use crate::error::{ApiResult, AppError};
use crate::http::AppState;
use crate::ingest::settings::IngestSettings;
use crate::oauth::{self, ConnectionStatus, Provider, SyncOutcome};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/oauth/status", get(status))
        .route("/oauth/:provider/authorize", get(authorize))
        .route("/oauth/:provider/callback", get(callback))
        .route("/oauth/:provider/sync", post(sync))
        .route("/oauth/:provider", axum::routing::delete(disconnect))
}

fn provider_of(s: &str) -> ApiResult<Provider> {
    Provider::parse(s).ok_or_else(|| AppError::NotFound("unknown provider".into()))
}

/// `GET /api/oauth/status` — per-provider configured/connected status for the current user.
async fn status(user: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<Vec<ConnectionStatus>>> {
    let list = oauth::status_for(&state.pool, &state.oauth, user.id)
        .await
        .map_err(AppError::Internal)?;
    Ok(Json(list))
}

/// `GET /api/oauth/:provider/authorize` — begin linking; returns the provider consent URL.
async fn authorize(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let provider = provider_of(&provider)?;
    let client = state
        .oauth
        .client(provider)
        .ok_or_else(|| AppError::BadRequest("this provider is not configured on the server".into()))?;
    let redirect_uri = state.oauth.redirect_uri(provider);
    let csrf = state.oauth_states.issue(user.id, provider);
    let url = oauth::authorize_url(provider, client, &redirect_uri, &csrf);
    Ok(Json(serde_json::json!({ "url": url })))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// `GET /api/oauth/:provider/callback` — provider redirect target. Exchanges the code for a refresh
/// token (stored encrypted) and bounces back to the SPA. The `state` param binds the flow to the
/// user who started it (CSRF); we never trust a client-supplied user id.
async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(q): Query<CallbackQuery>,
) -> Redirect {
    let base = state.oauth.redirect_base.trim_end_matches('/').to_string();
    let fail = |reason: &str| Redirect::to(&format!("{base}/settings?oauth_error={reason}"));

    let provider = match Provider::parse(&provider) {
        Some(p) => p,
        None => return fail("unknown_provider"),
    };
    if q.error.is_some() {
        return fail("denied");
    }
    let (Some(code), Some(csrf)) = (q.code, q.state) else {
        return fail("missing_code");
    };
    // Validate CSRF state and recover the initiating user; the provider in the state must match.
    let Some((user_id, expected)) = state.oauth_states.take(&csrf) else {
        return fail("bad_state");
    };
    if expected != provider {
        return fail("bad_state");
    }
    let Some(client) = state.oauth.client(provider) else {
        return fail("not_configured");
    };
    let redirect_uri = state.oauth.redirect_uri(provider);

    match oauth::exchange_code(&state.http_client, provider, client, &redirect_uri, &code).await {
        Ok((refresh_token, scope)) => {
            let label = oauth::fetch_account_label(&state.http_client, provider, client, &refresh_token).await;
            if let Err(e) = oauth::save_connection(
                &state.pool,
                &state.enc_key,
                user_id,
                provider,
                &refresh_token,
                scope.as_deref(),
                label.as_deref(),
            )
            .await
            {
                tracing::error!(error = ?e, "failed to store OAuth connection");
                return fail("store_failed");
            }
            Redirect::to(&format!("{base}/settings?connected={}", provider.as_str()))
        }
        Err(e) => {
            tracing::warn!(error = %e, provider = provider.as_str(), "OAuth code exchange failed");
            fail("exchange_failed")
        }
    }
}

#[derive(Deserialize)]
struct SyncBody {
    /// Category to import into; defaults to the user's `Other` when omitted.
    category_id: Option<i64>,
}

/// `POST /api/oauth/:provider/sync` — import the user's subscriptions, adding only new feeds. Safe
/// to press repeatedly.
async fn sync(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(provider): Path<String>,
    body: Option<Json<SyncBody>>,
) -> ApiResult<Json<SyncOutcome>> {
    let provider = provider_of(&provider)?;
    let client = state
        .oauth
        .client(provider)
        .ok_or_else(|| AppError::BadRequest("this provider is not configured on the server".into()))?;

    let refresh_token = oauth::load_refresh_token(&state.pool, &state.enc_key, user.id, provider)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::BadRequest("connect your account first".into()))?;

    let category_id = resolve_category(&state, user.id, body.and_then(|b| b.0.category_id)).await?;

    let subs = oauth::fetch_subscriptions(&state.http_client, provider, client, &refresh_token)
        .await
        .map_err(|e| AppError::Upstream(format!("could not fetch subscriptions: {e}")))?;

    let cfg = IngestSettings::load(&state.pool).await;
    let outcome = oauth::reconcile(&state.pool, &cfg, user.id, category_id, &subs).await?;

    oauth::touch_synced(&state.pool, user.id, provider).await.map_err(AppError::Internal)?;
    // Synced feeds are deliberately NOT due yet (§3 fix — no immediate backlog poll); they'll
    // join the normal polling schedule on their own. No scheduler wakeup needed here.
    Ok(Json(outcome))
}

/// `DELETE /api/oauth/:provider` — unlink the account (drops the stored token). Imported feeds stay.
async fn disconnect(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let provider = provider_of(&provider)?;
    oauth::delete_connection(&state.pool, user.id, provider).await.map_err(AppError::Internal)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Validate a provided category belongs to the user, or fall back to their `Other` category.
async fn resolve_category(state: &AppState, user_id: i64, provided: Option<i64>) -> ApiResult<i64> {
    if let Some(id) = provided {
        let ok = sqlx::query("SELECT 1 FROM categories WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await?
            .is_some();
        return if ok { Ok(id) } else { Err(AppError::BadRequest("a valid category is required".into())) };
    }
    sqlx::query("SELECT id FROM categories WHERE user_id = ? AND name = 'Other'")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
        .map(|r| r.get("id"))
        .ok_or_else(|| AppError::BadRequest("no default category found".into()))
}
