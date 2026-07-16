//! Server-side sessions (prompt.md §1a, §10). A random opaque id is stored in `sessions`
//! and carried in a **signed** cookie (SignedCookieJar, keyed by `SECRET_KEY`). Revocable on
//! logout, "logout everywhere", and user delete (FK cascade).

use anyhow::Result;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use axum_extra::extract::cookie::{Cookie, SameSite};
use sqlx::SqlitePool;

use super::SESSION_COOKIE;

/// Session lifetime.
const SESSION_TTL_DAYS: i64 = 30;

/// Create a new session row and return its opaque id (256-bit, hex).
pub async fn create(pool: &SqlitePool, user_id: i64) -> Result<String> {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let id = hex::encode(bytes);

    sqlx::query(
        "INSERT INTO sessions (id, user_id, expires_at)
         VALUES (?, ?, datetime('now', ?))",
    )
    .bind(&id)
    .bind(user_id)
    .bind(format!("+{SESSION_TTL_DAYS} days"))
    .execute(pool)
    .await?;

    Ok(id)
}

/// Delete a single session (logout).
pub async fn delete(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete every session for a user ("logout everywhere").
pub async fn delete_all_for_user(pool: &SqlitePool, user_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Build the session cookie. `http_only` + `SameSite::Lax`; not `Secure` so it also works over
/// plain HTTP on the LAN/dev (Tailscale provides HTTPS in production).
pub fn cookie(id: String) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, id))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(SESSION_TTL_DAYS))
        .build()
}

/// A removal cookie (path must match the one set above).
pub fn removal_cookie() -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, "")).path("/").build()
}
