//! Account self-service (prompt.md §9.12, §10): view profile, change password, rename, delete.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::SignedCookieJar;
use serde::Deserialize;
use sqlx::Row;

use super::UserDto;
use crate::auth::extract::CurrentUser;
use crate::auth::password::{hash_password, verify_password};
use crate::auth::{session, Role, ADMIN_USERNAME};
use crate::error::{ApiResult, AppError};
use crate::http::AppState;
use crate::routes::auth::{normalize_username, validate_username};

pub fn routes() -> Router<AppState> {
    Router::new().route("/me", get(get_me).patch(patch_me).delete(delete_me))
}

/// `GET /api/me` - the signed-in user (display username, role).
async fn get_me(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<UserDto>> {
    // CurrentUser.username is the normalized (canonical) form because ADMIN_USERNAME guards
    // depend on it. Re-select the display value for the response DTO.
    let username: String =
        sqlx::query("SELECT COALESCE(display_username, username) AS username FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_one(&state.pool)
            .await?
            .get("username");
    Ok(Json(UserDto {
        id: user.id,
        username,
        role: user.role,
    }))
}

#[derive(Deserialize)]
struct MeUpdate {
    current_password: Option<String>,
    new_password: Option<String>,
    username: Option<String>,
}

/// `PATCH /api/me` - rename this account and/or change password. At least one of `username` or
/// `new_password` must be set. Password change still requires `current_password`; rename does
/// NOT (the session is authentication, matching `delete_me` and admin role/disabled toggles).
///
/// Response is always the updated `UserDto` (display username), matching `login`/`register`.
async fn patch_me(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<MeUpdate>,
) -> ApiResult<Json<UserDto>> {
    if body.username.is_none() && body.new_password.is_none() {
        return Err(AppError::BadRequest("nothing to update".into()));
    }
    if let Some(u) = body.username.as_deref() {
        rename_username(&state, &user, u).await?;
    }
    if let Some(new_password) = body.new_password.as_deref() {
        change_password_for(&state, user.id, body.current_password.as_deref(), new_password).await?;
    }
    Ok(Json(load_me_dto(&state, user.id).await?))
}

/// Rename the current user. Rejects the built-in admin, validates the name, refuses collisions
/// (including rename-INTO "admin" from a non-admin), and maps a UNIQUE-violation on the UPDATE
/// to the same opaque `Conflict` as the pre-check. Sessions are NOT invalidated - `sessions.user_id`
/// is the FK, so the cookie keeps working and the next request re-reads the new name via
/// `CurrentUser`.
async fn rename_username(state: &AppState, user: &CurrentUser, raw_input: &str) -> ApiResult<()> {
    // The built-in admin account name is load-bearing (ADMIN_USERNAME guards); never let it
    // be renamed away.
    if user.username == ADMIN_USERNAME {
        return Err(AppError::Forbidden);
    }
    let raw = raw_input.trim().to_string();
    validate_username(&raw)?;
    let norm = normalize_username(&raw);
    // Renaming INTO "admin" is refused for everyone else too. Return the same opaque Conflict
    // as any other collision so we don't leak that "admin" is reserved.
    if norm == ADMIN_USERNAME {
        return Err(AppError::Conflict("username already taken".into()));
    }
    // Best-effort pre-check for a clearer error path; the UNIQUE-violation branch below closes
    // the race window.
    let taken = sqlx::query("SELECT 1 FROM users WHERE username = ? AND id != ? LIMIT 1")
        .bind(&norm)
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?
        .is_some();
    if taken {
        return Err(AppError::Conflict("username already taken".into()));
    }

    let res = sqlx::query("UPDATE users SET username = ?, display_username = ? WHERE id = ?")
        .bind(&norm)
        .bind(&raw)
        .bind(user.id)
        .execute(&state.pool)
        .await;
    match res {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(e)) if e.kind() == sqlx::error::ErrorKind::UniqueViolation => {
            Err(AppError::Conflict("username already taken".into()))
        }
        Err(e) => Err(e.into()),
    }
}

/// Change the given user's password. Requires `current_password` to match the stored hash and
/// `new_password` to be at least 8 characters. Behavior is verbatim to the pre-rename handler.
async fn change_password_for(
    state: &AppState,
    user_id: i64,
    current_password: Option<&str>,
    new_password: &str,
) -> ApiResult<()> {
    let current_password = current_password
        .ok_or_else(|| AppError::BadRequest("current password is incorrect".into()))?;

    let stored: Option<String> = sqlx::query("SELECT password_hash FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await?
        .get("password_hash");

    let ok = stored
        .as_deref()
        .map(|h| verify_password(current_password, h))
        .unwrap_or(false);
    if !ok {
        return Err(AppError::BadRequest("current password is incorrect".into()));
    }
    if new_password.chars().count() < 8 {
        return Err(AppError::BadRequest(
            "new password must be at least 8 characters".into(),
        ));
    }

    let hash = hash_password(new_password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&hash)
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

/// Read the current `UserDto` for `user_id` with display casing.
async fn load_me_dto(state: &AppState, user_id: i64) -> ApiResult<UserDto> {
    let row = sqlx::query(
        "SELECT id, COALESCE(display_username, username) AS username, role
         FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(UserDto {
        id: row.get("id"),
        username: row.get("username"),
        role: Role::from_db(row.get::<String, _>("role").as_str()),
    })
}

/// `DELETE /api/me` - delete own account (cascades all per-user data). Blocked for built-in admin.
async fn delete_me(
    State(state): State<AppState>,
    user: CurrentUser,
    jar: SignedCookieJar,
) -> ApiResult<(SignedCookieJar, Json<serde_json::Value>)> {
    if user.username == ADMIN_USERNAME {
        return Err(AppError::Forbidden);
    }
    // Sessions cascade via FK, but delete explicitly first to be safe, then the user.
    session::delete_all_for_user(&state.pool, user.id).await?;
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user.id)
        .execute(&state.pool)
        .await?;
    let jar = jar.remove(session::removal_cookie());
    Ok((jar, Json(serde_json::json!({ "ok": true }))))
}
