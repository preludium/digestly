//! Per-user scoping extractors - the shared helper every handler uses (prompt.md §10, §11).
//!
//! `CurrentUser` resolves `user_id` FROM THE SESSION, never a client parameter. Handlers that
//! touch per-user rows take `CurrentUser` and filter by `user.id`. `AdminUser` additionally
//! enforces `role = admin` at the server (403), not just hidden UI.

use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::cookie::SignedCookieJar;
use sqlx::Row;

use super::{Role, SESSION_COOKIE};
use crate::error::AppError;
use crate::http::AppState;

/// The authenticated caller, resolved from the signed session cookie.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub role: Role,
}

impl CurrentUser {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}

#[async_trait]
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let jar = SignedCookieJar::from_headers(&parts.headers, state.key.clone());
        let sid = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_string())
            .ok_or(AppError::Unauthorized)?;

        let row = sqlx::query(
            "SELECT u.id AS id, u.username AS username, u.role AS role, u.disabled AS disabled
             FROM sessions s
             JOIN users u ON u.id = s.user_id
             WHERE s.id = ? AND s.expires_at > datetime('now')",
        )
        .bind(&sid)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::Unauthorized)?;

        let disabled: i64 = row.get("disabled");
        if disabled != 0 {
            return Err(AppError::Unauthorized);
        }

        Ok(CurrentUser {
            id: row.get("id"),
            username: row.get("username"),
            role: Role::from_db(row.get::<String, _>("role").as_str()),
        })
    }
}

/// An authenticated caller that must be an admin (server-enforced 403). Wraps the resolved
/// `CurrentUser` so admin handlers can read the admin's identity; extracting it is the point,
/// even where a handler only needs the guard.
#[derive(Debug, Clone)]
pub struct AdminUser(#[allow(dead_code)] pub CurrentUser);

#[async_trait]
impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(AppError::Forbidden);
        }
        Ok(AdminUser(user))
    }
}
