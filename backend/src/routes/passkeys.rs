//! Passkey / WebAuthn endpoints (prompt.md §9.10, §9.12, §10, §11 - Stretch S1).
//!
//! Two families:
//! * **Passwordless login** (`/api/auth/passkey/login/{options,verify}`) - public; username-first
//!   so the RP can name the allowed credentials, then a WebAuthn assertion signs the user in.
//!   A discoverable (Conditional UI / autofill) variant lives at
//!   `/api/auth/passkey/discoverable/login/{options,verify}` - no username needed; the user is
//!   resolved from the chosen credential's embedded handle.
//! * **Management** (`/api/passkeys/*`) - authed; register a new passkey, list/rename/delete.
//!
//! Ceremony state lives in-process ([`CeremonyStore`]) between the `options` and `verify` calls;
//! the client echoes back the opaque `ceremony_id` it received. Sign-count regression and the
//! last-sign-in-method guard are enforced here (and mirrored by `webauthn-rs`).

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::SignedCookieJar;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use webauthn_rs::prelude::*;

use super::UserDto;
use crate::auth::extract::CurrentUser;
use crate::auth::passkey::{self, Ceremony};
use crate::auth::{session, Role};
use crate::error::{ApiResult, AppError};
use crate::http::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        // Passwordless login (public).
        .route("/auth/passkey/login/options", post(login_options))
        .route("/auth/passkey/login/verify", post(login_verify))
        // Discoverable / Conditional-UI login (public, autofill).
        .route(
            "/auth/passkey/discoverable/login/options",
            post(discoverable_login_options),
        )
        .route(
            "/auth/passkey/discoverable/login/verify",
            post(discoverable_login_verify),
        )
        // Management (authed).
        .route("/passkeys", get(list))
        .route("/passkeys/register/options", post(register_options))
        .route("/passkeys/register/verify", post(register_verify))
        .route("/passkeys/:id", axum::routing::patch(rename).delete(delete))
}

/// Pull the configured RP, or a clear 400 if passkeys are disabled (bad RP config).
fn rp(state: &AppState) -> ApiResult<&Webauthn> {
    state
        .webauthn
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("passkeys are not enabled on this server".into()))
}

// ── Registration (authed) ────────────────────────────────────────────────────

#[derive(Serialize)]
struct CeremonyResponse<T> {
    ceremony_id: String,
    options: T,
}

/// `POST /api/passkeys/register/options` - begin registering a passkey for the current user.
/// Excludes the user's existing credentials so the same authenticator can't be double-registered.
async fn register_options(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<CeremonyResponse<CreationChallengeResponse>>> {
    let webauthn = rp(&state)?;

    // Credentials to exclude = everything the user already has.
    let existing = load_user_passkeys(&state, user.id).await?;
    let exclude: Vec<CredentialID> = existing.iter().map(|p| p.cred_id().clone()).collect();
    let exclude = if exclude.is_empty() {
        None
    } else {
        Some(exclude)
    };

    // WebAuthn `name` is the RP-facing account handle (canonical, matches every ADMIN_USERNAME
    // guard elsewhere); `display_name` is what the OS shows in the prompt and should reflect
    // the user-chosen casing. Both are frozen inside the authenticator at credential creation -
    // a later username rename does NOT update authenticator-side metadata.
    let display_name: String =
        sqlx::query("SELECT COALESCE(display_username, username) AS n FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_one(&state.pool)
            .await?
            .get("n");
    let (options, reg_state) = webauthn
        .start_passkey_registration(
            passkey::user_handle(user.id),
            &user.username,
            &display_name,
            exclude,
        )
        .map_err(webauthn_bad_request)?;

    let ceremony_id = state.passkey_ceremonies.insert(Ceremony::Register {
        user_id: user.id,
        state: reg_state,
    });
    Ok(Json(CeremonyResponse {
        ceremony_id,
        options,
    }))
}

#[derive(Deserialize)]
struct RegisterVerify {
    ceremony_id: String,
    credential: RegisterPublicKeyCredential,
    name: Option<String>,
}

#[derive(Serialize)]
struct PasskeyDto {
    id: i64,
    name: String,
    created_at: String,
    last_used_at: Option<String>,
}

/// `POST /api/passkeys/register/verify` - finish registration and persist the credential.
async fn register_verify(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<RegisterVerify>,
) -> ApiResult<Json<PasskeyDto>> {
    let webauthn = rp(&state)?;

    let ceremony = state
        .passkey_ceremonies
        .take(&body.ceremony_id)
        .ok_or_else(|| {
            AppError::BadRequest("passkey registration expired - please try again".into())
        })?;
    let Ceremony::Register {
        user_id,
        state: reg_state,
    } = ceremony
    else {
        return Err(AppError::BadRequest("wrong ceremony type".into()));
    };
    // The ceremony must belong to the caller (defence in depth; the id is unguessable already).
    if user_id != user.id {
        return Err(AppError::Forbidden);
    }

    let passkey = webauthn
        .finish_passkey_registration(&body.credential, &reg_state)
        .map_err(webauthn_bad_request)?;

    let cred_key = passkey::credential_key(passkey.cred_id());
    let blob = passkey::serialize_credential(&passkey).map_err(AppError::Internal)?;
    let name = clean_name(body.name).unwrap_or_else(|| "Passkey".to_string());

    // UNIQUE(credential_id) guards against re-registering the same authenticator.
    let existing = sqlx::query("SELECT 1 FROM passkeys WHERE credential_id = ?")
        .bind(&cred_key)
        .fetch_optional(&state.pool)
        .await?;
    if existing.is_some() {
        return Err(AppError::Conflict(
            "this passkey is already registered".into(),
        ));
    }

    let row = sqlx::query(
        "INSERT INTO passkeys (user_id, credential_id, public_key, sign_count, name)
         VALUES (?, ?, ?, 0, ?)
         RETURNING id, name, created_at, last_used_at",
    )
    .bind(user.id)
    .bind(&cred_key)
    .bind(&blob)
    .bind(&name)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(PasskeyDto {
        id: row.get("id"),
        name: row.get("name"),
        created_at: row.get("created_at"),
        last_used_at: row.get("last_used_at"),
    }))
}

// ── Passwordless login (public) ──────────────────────────────────────────────

#[derive(Deserialize)]
struct LoginOptions {
    username: String,
}

/// `POST /api/auth/passkey/login/options` - begin a passwordless sign-in for a username. The RP
/// needs the account's credentials to build the assertion challenge, so this is username-first
/// (still passwordless). A clear error if the account has no passkeys.
async fn login_options(
    State(state): State<AppState>,
    Json(body): Json<LoginOptions>,
) -> ApiResult<Json<CeremonyResponse<RequestChallengeResponse>>> {
    let webauthn = rp(&state)?;
    // Share the exact same trim+Unicode-lowercase pass used by password login and register
    // (`crate::routes::auth::normalize_username`) so the two flows can't drift.
    let username = crate::routes::auth::normalize_username(&body.username);

    // Resolve the user (must be enabled). Errors here are intentionally the same shape.
    let row = sqlx::query("SELECT id, disabled FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&state.pool)
        .await?;
    let (user_id, disabled): (i64, i64) = match row {
        Some(r) => (r.get("id"), r.get("disabled")),
        None => {
            return Err(AppError::BadRequest(
                "no passkey is registered for this account".into(),
            ))
        }
    };
    if disabled != 0 {
        return Err(AppError::Unauthorized);
    }

    let passkeys = load_user_passkeys(&state, user_id).await?;
    if passkeys.is_empty() {
        return Err(AppError::BadRequest(
            "no passkey is registered for this account".into(),
        ));
    }

    let (options, auth_state) = webauthn
        .start_passkey_authentication(&passkeys)
        .map_err(webauthn_bad_request)?;

    let ceremony_id = state.passkey_ceremonies.insert(Ceremony::Login {
        user_id,
        state: auth_state,
    });
    Ok(Json(CeremonyResponse {
        ceremony_id,
        options,
    }))
}

#[derive(Deserialize)]
struct LoginVerify {
    ceremony_id: String,
    credential: PublicKeyCredential,
}

/// `POST /api/auth/passkey/login/verify` - finish the assertion, enforce sign-count regression,
/// and issue a session. Generic `Unauthorized` on any failure (no enumeration).
async fn login_verify(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Json(body): Json<LoginVerify>,
) -> ApiResult<(SignedCookieJar, Json<UserDto>)> {
    let webauthn = rp(&state)?;

    let ceremony = state
        .passkey_ceremonies
        .take(&body.ceremony_id)
        .ok_or(AppError::Unauthorized)?;
    let Ceremony::Login {
        user_id,
        state: auth_state,
    } = ceremony
    else {
        return Err(AppError::Unauthorized);
    };

    // `webauthn-rs` verifies the assertion signature and its own counter check here.
    let result = webauthn
        .finish_passkey_authentication(&body.credential, &auth_state)
        .map_err(|_| AppError::Unauthorized)?;

    finish_login(&state, jar, user_id, &result).await
}

// ── Discoverable / Conditional-UI login (public) ─────────────────────────────

/// `POST /api/auth/passkey/discoverable/login/options` - begin an autofill (Conditional UI)
/// sign-in. No username is required: the crate emits `mediation: "conditional"` into the
/// challenge, and the authenticator later reveals which credential (and user) was chosen.
async fn discoverable_login_options(
    State(state): State<AppState>,
) -> ApiResult<Json<CeremonyResponse<RequestChallengeResponse>>> {
    let webauthn = rp(&state)?;

    let (options, auth_state) = webauthn
        .start_discoverable_authentication()
        .map_err(webauthn_bad_request)?;

    let ceremony_id = state
        .passkey_ceremonies
        .insert(Ceremony::DiscoverableLogin { state: auth_state });
    Ok(Json(CeremonyResponse {
        ceremony_id,
        options,
    }))
}

/// `POST /api/auth/passkey/discoverable/login/verify` - resolve the user from the discoverable
/// credential, verify the assertion, and issue a session. Generic `Unauthorized` on any failure.
async fn discoverable_login_verify(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Json(body): Json<LoginVerify>,
) -> ApiResult<(SignedCookieJar, Json<UserDto>)> {
    let webauthn = rp(&state)?;

    let ceremony = state
        .passkey_ceremonies
        .take(&body.ceremony_id)
        .ok_or(AppError::Unauthorized)?;
    let Ceremony::DiscoverableLogin { state: auth_state } = ceremony else {
        return Err(AppError::Unauthorized);
    };

    // Identify which user's credential was presented, without ever asking for a username. The
    // handle was embedded at registration via `passkey::user_handle(user_id)`; decode it back.
    let (user_uuid, _cred_id) = webauthn
        .identify_discoverable_authentication(&body.credential)
        .map_err(|_| AppError::Unauthorized)?;
    let user_id = user_uuid.as_u128() as i64;

    // Rehydrate that user's credentials as discoverable keys for the crate's verification.
    let passkeys = load_user_passkeys(&state, user_id).await?;
    if passkeys.is_empty() {
        return Err(AppError::Unauthorized);
    }
    let creds: Vec<DiscoverableKey> = passkeys.iter().map(DiscoverableKey::from).collect();

    let result = webauthn
        .finish_discoverable_authentication(&body.credential, auth_state, &creds)
        .map_err(|_| AppError::Unauthorized)?;

    finish_login(&state, jar, user_id, &result).await
}

/// Shared post-authentication bookkeeping for both the username-first and discoverable login
/// flows: sign-count regression guard (§11), credential counter/public-key sync, `last_used_at`
/// / `last_login_at` updates, and session + cookie issuance. Generic `Unauthorized` on failure.
async fn finish_login(
    state: &AppState,
    jar: SignedCookieJar,
    user_id: i64,
    result: &AuthenticationResult,
) -> ApiResult<(SignedCookieJar, Json<UserDto>)> {
    let cred_key = passkey::credential_key(result.cred_id());
    let stored = sqlx::query(
        "SELECT id, public_key, sign_count FROM passkeys WHERE credential_id = ? AND user_id = ?",
    )
    .bind(&cred_key)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::Unauthorized)?;

    let passkey_id: i64 = stored.get("id");
    let stored_count: i64 = stored.get("sign_count");
    let presented = result.counter();

    // Explicit sign-count regression guard (§11) - reject a cloned/replayed authenticator.
    if passkey::sign_count_regressed(stored_count as u32, presented) {
        tracing::warn!(
            user_id,
            passkey_id,
            stored_count,
            presented,
            "rejecting passkey: sign-count regression (possible cloned authenticator)"
        );
        return Err(AppError::Unauthorized);
    }

    // Keep the stored credential's counter in sync with the library's view.
    if result.needs_update() {
        let blob: Vec<u8> = stored.get("public_key");
        if let Ok(mut pk) = passkey::deserialize_credential(&blob) {
            pk.update_credential(result);
            if let Ok(updated) = passkey::serialize_credential(&pk) {
                let _ = sqlx::query("UPDATE passkeys SET public_key = ? WHERE id = ?")
                    .bind(&updated)
                    .bind(passkey_id)
                    .execute(&state.pool)
                    .await;
            }
        }
    }
    sqlx::query("UPDATE passkeys SET sign_count = ?, last_used_at = datetime('now') WHERE id = ?")
        .bind(presented as i64)
        .bind(passkey_id)
        .execute(&state.pool)
        .await?;

    // Resolve identity and issue a session (same shape as password login).
    let urow = sqlx::query(
        "SELECT id, COALESCE(display_username, username) AS username, role
         FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let role = Role::from_db(urow.get::<String, _>("role").as_str());
    sqlx::query("UPDATE users SET last_login_at = datetime('now') WHERE id = ?")
        .bind(user_id)
        .execute(&state.pool)
        .await?;

    let sid = session::create(&state.pool, user_id).await?;
    let jar = jar.add(session::cookie(sid));
    Ok((
        jar,
        Json(UserDto {
            id: user_id,
            username: urow.get("username"),
            role,
        }),
    ))
}

// ── Management (authed) ──────────────────────────────────────────────────────

/// `GET /api/passkeys` - the current user's passkeys (never the public-key material).
async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Vec<PasskeyDto>>> {
    let rows = sqlx::query(
        "SELECT id, name, created_at, last_used_at FROM passkeys WHERE user_id = ? ORDER BY id",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    let list = rows
        .into_iter()
        .map(|r| PasskeyDto {
            id: r.get("id"),
            name: r
                .get::<Option<String>, _>("name")
                .unwrap_or_else(|| "Passkey".into()),
            created_at: r.get("created_at"),
            last_used_at: r.get("last_used_at"),
        })
        .collect();
    Ok(Json(list))
}

#[derive(Deserialize)]
struct Rename {
    name: String,
}

/// `PATCH /api/passkeys/:id` - rename one of the current user's passkeys.
async fn rename(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<Rename>,
) -> ApiResult<Json<serde_json::Value>> {
    let name = clean_name(Some(body.name))
        .ok_or_else(|| AppError::BadRequest("name cannot be empty".into()))?;
    let res = sqlx::query("UPDATE passkeys SET name = ? WHERE id = ? AND user_id = ?")
        .bind(&name)
        .bind(id)
        .bind(user.id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("passkey not found".into()));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/passkeys/:id` - remove a passkey, unless it's the user's only sign-in method.
async fn delete(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    // The target must belong to the caller.
    let owned = sqlx::query("SELECT 1 FROM passkeys WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?;
    if owned.is_none() {
        return Err(AppError::NotFound("passkey not found".into()));
    }

    let has_password: bool = sqlx::query("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?
        .get::<Option<String>, _>("password_hash")
        .is_some();
    let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM passkeys WHERE user_id = ?")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?
        .get("n");

    if passkey::would_orphan_account(has_password, count) {
        return Err(AppError::BadRequest(
            "cannot remove your only sign-in method - set a password or add another passkey first"
                .into(),
        ));
    }

    sqlx::query("DELETE FROM passkeys WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Load and rehydrate all of a user's stored credentials.
async fn load_user_passkeys(state: &AppState, user_id: i64) -> ApiResult<Vec<Passkey>> {
    let rows = sqlx::query("SELECT public_key FROM passkeys WHERE user_id = ?")
        .bind(user_id)
        .fetch_all(&state.pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let blob: Vec<u8> = r.get("public_key");
        // A credential we can't decode (e.g. after an incompatible upgrade) is skipped, not fatal.
        if let Ok(pk) = passkey::deserialize_credential(&blob) {
            out.push(pk);
        }
    }
    Ok(out)
}

fn clean_name(name: Option<String>) -> Option<String> {
    let n = name?.trim().chars().take(64).collect::<String>();
    if n.is_empty() {
        None
    } else {
        Some(n)
    }
}

/// Map a WebAuthn ceremony error to a safe 400 (no internal detail leaks to the client).
fn webauthn_bad_request(e: WebauthnError) -> AppError {
    tracing::warn!(error = %e, "webauthn ceremony error");
    AppError::BadRequest("passkey operation failed".into())
}
