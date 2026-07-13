//! Account self-service (prompt.md §9.12, §10): view profile, change password, delete account.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::SignedCookieJar;
use serde::Deserialize;
use sqlx::Row;

use super::UserDto;
use crate::auth::extract::CurrentUser;
use crate::auth::password::{hash_password, verify_password};
use crate::auth::{session, ADMIN_USERNAME};
use crate::error::{ApiResult, AppError};
use crate::http::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/me", get(get_me).patch(change_password).delete(delete_me))
}

/// `GET /api/me` - the signed-in user (username, role).
async fn get_me(user: CurrentUser) -> Json<UserDto> {
    Json(UserDto {
        id: user.id,
        username: user.username,
        role: user.role,
    })
}

#[derive(Deserialize)]
struct ChangePassword {
    current_password: String,
    new_password: String,
}

/// `PATCH /api/me` - change password; requires the current password.
async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<ChangePassword>,
) -> ApiResult<Json<serde_json::Value>> {
    let stored: Option<String> = sqlx::query("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?
        .get("password_hash");

    let ok = stored
        .as_deref()
        .map(|h| verify_password(&body.current_password, h))
        .unwrap_or(false);
    if !ok {
        return Err(AppError::BadRequest("current password is incorrect".into()));
    }
    if body.new_password.chars().count() < 8 {
        return Err(AppError::BadRequest(
            "new password must be at least 8 characters".into(),
        ));
    }

    let hash = hash_password(&body.new_password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&hash)
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
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
