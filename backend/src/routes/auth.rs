//! Auth endpoints (prompt.md §9.10, §9.10a, §10, §11): register / login / logout, plus a
//! public flag telling the UI whether to show the register link.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::SignedCookieJar;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use super::UserDto;
use crate::auth::password::{hash_password, verify_password};
use crate::auth::{session, Role};
use crate::error::{ApiResult, AppError};
use crate::http::AppState;
use crate::seed::seed_default_categories;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-all", post(logout_all))
        .route("/auth/registration", get(registration_status))
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct RegistrationStatus {
    allow_registration: bool,
    /// Whether passkey sign-in is available (RP configured). Drives the login-page button (§9.10).
    passkeys_enabled: bool,
}

/// Public: whether open self-registration is currently enabled (§1a) and whether passkeys are on.
async fn registration_status(State(state): State<AppState>) -> ApiResult<Json<RegistrationStatus>> {
    Ok(Json(RegistrationStatus {
        allow_registration: allow_registration(&state.pool).await?,
        passkeys_enabled: state.webauthn.is_some(),
    }))
}

/// `POST /api/auth/register` - open self-signup (role=user), gated by `allow_registration`.
async fn register(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Json(body): Json<Credentials>,
) -> ApiResult<(SignedCookieJar, Json<UserDto>)> {
    if !allow_registration(&state.pool).await? {
        return Err(AppError::RegistrationDisabled);
    }
    let raw = body.username.trim().to_string();
    validate_username(&raw)?;
    let username = normalize_username(&body.username);
    validate_password(&body.password)?;

    let taken = sqlx::query("SELECT 1 FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&state.pool)
        .await?
        .is_some();
    if taken {
        return Err(AppError::Conflict("username already taken".into()));
    }

    let hash = hash_password(&body.password)?;
    // Race between the pre-check above and the INSERT: UNIQUE(username) closes the window, but
    // sqlx's default `From<sqlx::Error>` renders it as 500. Map the UNIQUE-constraint case
    // explicitly to the same 409 the pre-check would surface.
    let insert = sqlx::query(
        "INSERT INTO users (username, display_username, password_hash, role, last_login_at)
         VALUES (?, ?, ?, ?, datetime('now')) RETURNING id",
    )
    .bind(&username)
    .bind(&raw)
    .bind(&hash)
    .bind(Role::User.as_str())
    .fetch_one(&state.pool)
    .await;
    let id: i64 = match insert {
        Ok(row) => row.get("id"),
        Err(sqlx::Error::Database(e)) if e.kind() == sqlx::error::ErrorKind::UniqueViolation => {
            return Err(AppError::Conflict("username already taken".into()));
        }
        Err(e) => return Err(e.into()),
    };

    seed_default_categories(&state.pool, id).await?;

    let sid = session::create(&state.pool, id).await?;
    let jar = jar.add(session::cookie(sid));
    Ok((
        jar,
        Json(UserDto {
            id,
            username: raw,
            role: Role::User,
        }),
    ))
}

/// `POST /api/auth/login` - username + password. Generic errors (no username enumeration).
async fn login(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Json(body): Json<Credentials>,
) -> ApiResult<(SignedCookieJar, Json<UserDto>)> {
    let username = normalize_username(&body.username);
    let row = sqlx::query(
        "SELECT id, COALESCE(display_username, username) AS username, role, disabled, password_hash
         FROM users WHERE username = ?",
    )
    .bind(&username)
    .fetch_optional(&state.pool)
    .await?;

    // Same error for unknown user, wrong password, or disabled account (no enumeration).
    let row = row.ok_or(AppError::Unauthorized)?;
    let disabled: i64 = row.get("disabled");
    let stored: Option<String> = row.get("password_hash");
    let ok = stored
        .as_deref()
        .map(|h| verify_password(&body.password, h))
        .unwrap_or(false);
    if !ok || disabled != 0 {
        return Err(AppError::Unauthorized);
    }

    let id: i64 = row.get("id");
    let role = Role::from_db(row.get::<String, _>("role").as_str());
    sqlx::query("UPDATE users SET last_login_at = datetime('now') WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;

    let sid = session::create(&state.pool, id).await?;
    let jar = jar.add(session::cookie(sid));
    Ok((
        jar,
        Json(UserDto {
            id,
            username: row.get("username"),
            role,
        }),
    ))
}

/// `POST /api/auth/logout` - revoke this session and clear the cookie.
async fn logout(
    State(state): State<AppState>,
    jar: SignedCookieJar,
) -> ApiResult<(SignedCookieJar, Json<serde_json::Value>)> {
    if let Some(c) = jar.get(crate::auth::SESSION_COOKIE) {
        session::delete(&state.pool, c.value()).await?;
    }
    let jar = jar.remove(session::removal_cookie());
    Ok((jar, Json(serde_json::json!({ "ok": true }))))
}

/// `POST /api/auth/logout-all` - revoke every session for the current user (§9.12).
async fn logout_all(
    State(state): State<AppState>,
    user: crate::auth::extract::CurrentUser,
    jar: SignedCookieJar,
) -> ApiResult<(SignedCookieJar, Json<serde_json::Value>)> {
    session::delete_all_for_user(&state.pool, user.id).await?;
    let jar = jar.remove(session::removal_cookie());
    Ok((jar, Json(serde_json::json!({ "ok": true }))))
}

/// Read the global `allow_registration` flag (defaults to true if unset).
pub async fn allow_registration(pool: &SqlitePool) -> ApiResult<bool> {
    let val: Option<String> =
        sqlx::query("SELECT value FROM app_settings WHERE key = 'allow_registration'")
            .fetch_optional(pool)
            .await?
            .map(|r| r.get("value"));
    Ok(val.map(|v| v == "true").unwrap_or(true))
}

/// Trim + full-Unicode lowercase. Canonical form for storage, uniqueness, and
/// `ADMIN_USERNAME` guards. Not NFC-normalized: decomposed sequences are rejected upstream
/// via `validate_username`, so we never see a combining mark reach this point.
pub(crate) fn normalize_username(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Enforce the username invariant on both the raw trimmed input and its normalized form.
///
/// Length: `chars().count() ∈ [3, 32]` on BOTH sides. Rust's `to_lowercase()` is Unicode-aware
/// and can change the char count for exotic codepoints (e.g. "İ" U+0130 → "i" + U+0307), so a
/// 32-char raw could normalize to 33 chars and silently slip past a one-sided check.
///
/// Charset (checked on the normalized form): `c.is_alphanumeric() || matches!(c, '.' | '_' | '-')`.
/// `is_alphanumeric()` is Unicode-aware and returns `true` for letters/digits from all scripts
/// (e.g. "Łukasz"), but `false` for category-Mn combining marks (U+0301 COMBINING ACUTE ACCENT)
/// and category-Cf format chars (U+200D ZERO WIDTH JOINER). We deliberately do NOT apply NFC
/// normalization here - the `unicode-normalization` crate is not a dependency, and rejecting
/// decomposed input keeps us from silently rewriting user-facing bytes.
pub(crate) fn validate_username(raw: &str) -> ApiResult<()> {
    let trimmed = raw.trim();
    let raw_len = trimmed.chars().count();
    if !(3..=32).contains(&raw_len) {
        return Err(AppError::BadRequest(
            "username must be 3–32 characters".into(),
        ));
    }
    let norm = normalize_username(trimmed);
    let norm_len = norm.chars().count();
    if !(3..=32).contains(&norm_len) {
        return Err(AppError::BadRequest(
            "username must be 3–32 characters".into(),
        ));
    }
    if !norm
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(AppError::BadRequest(
            "username may contain only letters, digits, and . _ -".into(),
        ));
    }
    Ok(())
}

fn validate_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < 8 {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    Ok(())
}
