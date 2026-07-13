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
    let username = normalize_username(&body.username);
    validate_username(&username)?;
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
    let id: i64 = sqlx::query(
        "INSERT INTO users (username, password_hash, role, last_login_at)
         VALUES (?, ?, ?, datetime('now')) RETURNING id",
    )
    .bind(&username)
    .bind(&hash)
    .bind(Role::User.as_str())
    .fetch_one(&state.pool)
    .await?
    .get("id");

    seed_default_categories(&state.pool, id).await?;

    let sid = session::create(&state.pool, id).await?;
    let jar = jar.add(session::cookie(sid));
    Ok((
        jar,
        Json(UserDto {
            id,
            username,
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
        "SELECT id, username, role, disabled, password_hash FROM users WHERE username = ?",
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

fn normalize_username(raw: &str) -> String {
    raw.trim().to_lowercase()
}

fn validate_username(username: &str) -> ApiResult<()> {
    let len = username.chars().count();
    if !(3..=32).contains(&len) {
        return Err(AppError::BadRequest(
            "username must be 3–32 characters".into(),
        ));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
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
