//! Startup bootstrap (prompt.md §1a, §11): ensure the built-in `admin` exists with the
//! `ADMIN_PASSWORD` hash (re-synced if the env value changed) and that global defaults exist.

use anyhow::Result;
use sqlx::{Row, SqlitePool};
use tracing::info;

use super::password::hash_password;
use super::{Role, ADMIN_USERNAME};
use crate::seed::seed_default_categories;

/// Idempotent boot routine: default `app_settings`, then the admin account.
pub async fn run(pool: &SqlitePool, admin_password: &str) -> Result<()> {
    ensure_default_settings(pool).await?;
    ensure_admin(pool, admin_password).await?;
    Ok(())
}

/// Seed admin-only global defaults if absent (open registration on by default).
async fn ensure_default_settings(pool: &SqlitePool) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO app_settings (key, value) VALUES ('allow_registration', 'true')")
        .execute(pool)
        .await?;
    Ok(())
}

/// Ensure the built-in admin exists; re-hash the password from env on every boot so a changed
/// `ADMIN_PASSWORD` re-syncs. Always role=admin and enabled.
async fn ensure_admin(pool: &SqlitePool, admin_password: &str) -> Result<()> {
    let hash = hash_password(admin_password)?;

    let existing = sqlx::query("SELECT id FROM users WHERE username = ?")
        .bind(ADMIN_USERNAME)
        .fetch_optional(pool)
        .await?;

    match existing {
        Some(row) => {
            let id: i64 = row.get("id");
            sqlx::query(
                "UPDATE users SET password_hash = ?, role = ?, disabled = 0 WHERE id = ?",
            )
            .bind(&hash)
            .bind(Role::Admin.as_str())
            .bind(id)
            .execute(pool)
            .await?;
            seed_default_categories(pool, id).await?;
            info!("admin account present; password hash re-synced from ADMIN_PASSWORD");
        }
        None => {
            let id: i64 = sqlx::query(
                "INSERT INTO users (username, password_hash, role) VALUES (?, ?, ?) RETURNING id",
            )
            .bind(ADMIN_USERNAME)
            .bind(&hash)
            .bind(Role::Admin.as_str())
            .fetch_one(pool)
            .await?
            .get("id");
            seed_default_categories(pool, id).await?;
            info!("bootstrapped built-in admin account from ADMIN_PASSWORD");
        }
    }
    Ok(())
}
