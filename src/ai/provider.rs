//! `ai_providers` persistence (prompt.md §2, §6). Admin-managed, global, exactly one active.
//! Keys are encrypted at rest ([`crypto`](super::crypto)) and **never** read back out to the API —
//! only decrypted in-process for a live call. The only key mutation is delete+create.

use anyhow::Result;
use serde::Serialize;
use sqlx::{Row, SqlitePool};

use super::{crypto, ApiStyle};

/// A provider row as returned to the admin UI — **never** includes the key (only `has_key`).
#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub id: i64,
    pub name: String,
    pub provider_type: String,
    pub api_style: String,
    pub base_url: String,
    pub model: String,
    pub has_key: bool,
    pub is_active: bool,
}

/// A provider resolved for a live call, with its decrypted key held only in memory.
pub struct ResolvedProvider {
    pub api_style: ApiStyle,
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
}

/// Fields accepted on create (prompt.md §6, §10). `key` is plaintext on the way in only.
pub struct NewProvider {
    pub name: String,
    pub provider_type: String,
    pub api_style: ApiStyle,
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
}

/// List all providers without keys (prompt.md §10 `GET /api/ai/providers`).
pub async fn list(pool: &SqlitePool) -> Result<Vec<ProviderInfo>> {
    let rows = sqlx::query(
        "SELECT id, name, provider_type, api_style, base_url, model,
                (api_key_enc IS NOT NULL) AS has_key, is_active
         FROM ai_providers ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ProviderInfo {
            id: r.get("id"),
            name: r.get("name"),
            provider_type: r.get("provider_type"),
            api_style: r.get("api_style"),
            base_url: r.get("base_url"),
            model: r.get("model"),
            has_key: r.get::<i64, _>("has_key") != 0,
            is_active: r.get::<i64, _>("is_active") != 0,
        })
        .collect())
}

/// Create a provider, encrypting the key at rest. Auto-activates it when it's the first provider
/// so the instance always has an active provider after the first add.
pub async fn create(pool: &SqlitePool, enc_key: &[u8; 32], p: NewProvider) -> Result<i64> {
    let key_enc = match p.key.as_deref().filter(|k| !k.trim().is_empty()) {
        Some(k) => Some(crypto::encrypt(enc_key, k)?),
        None => None,
    };

    let none_active: i64 =
        sqlx::query("SELECT COUNT(*) AS n FROM ai_providers WHERE is_active = 1")
            .fetch_one(pool)
            .await?
            .get("n");
    let activate = none_active == 0;

    let id: i64 = sqlx::query(
        "INSERT INTO ai_providers (name, provider_type, api_style, base_url, model, api_key_enc, is_active)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&p.name)
    .bind(&p.provider_type)
    .bind(p.api_style.as_str())
    .bind(&p.base_url)
    .bind(&p.model)
    .bind(key_enc)
    .bind(activate as i64)
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}

/// Edit `name`/`model` only — never the key (prompt.md §6, §10).
pub async fn patch(pool: &SqlitePool, id: i64, name: Option<&str>, model: Option<&str>) -> Result<bool> {
    let exists = exists(pool, id).await?;
    if !exists {
        return Ok(false);
    }
    if let Some(name) = name {
        sqlx::query("UPDATE ai_providers SET name = ? WHERE id = ?").bind(name).bind(id).execute(pool).await?;
    }
    if let Some(model) = model {
        sqlx::query("UPDATE ai_providers SET model = ? WHERE id = ?").bind(model).bind(id).execute(pool).await?;
    }
    Ok(true)
}

/// Make `id` the single active provider (instance-wide).
pub async fn activate(pool: &SqlitePool, id: i64) -> Result<bool> {
    if !exists(pool, id).await? {
        return Ok(false);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE ai_providers SET is_active = 0").execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_providers SET is_active = 1 WHERE id = ?").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}

/// Delete a provider (rotating a key = delete + create).
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool> {
    let n = sqlx::query("DELETE FROM ai_providers WHERE id = ?").bind(id).execute(pool).await?.rows_affected();
    Ok(n > 0)
}

/// The active provider with its key decrypted in memory (`None` if none is active).
pub async fn load_active(pool: &SqlitePool, enc_key: &[u8; 32]) -> Result<Option<ResolvedProvider>> {
    let row = sqlx::query(
        "SELECT id, name, api_style, base_url, model, api_key_enc
         FROM ai_providers WHERE is_active = 1 LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    resolve(row, enc_key)
}

/// A specific provider with its key decrypted in memory (for `POST .../{id}/test`).
pub async fn load_one(pool: &SqlitePool, enc_key: &[u8; 32], id: i64) -> Result<Option<ResolvedProvider>> {
    let row = sqlx::query(
        "SELECT id, name, api_style, base_url, model, api_key_enc FROM ai_providers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    resolve(row, enc_key)
}

fn resolve(row: Option<sqlx::sqlite::SqliteRow>, enc_key: &[u8; 32]) -> Result<Option<ResolvedProvider>> {
    let Some(r) = row else { return Ok(None) };
    let enc: Option<Vec<u8>> = r.get("api_key_enc");
    let key = match enc {
        Some(blob) => Some(crypto::decrypt(enc_key, &blob)?),
        None => None,
    };
    let style = ApiStyle::parse(r.get::<String, _>("api_style").as_str())
        .ok_or_else(|| anyhow::anyhow!("stored provider has an invalid api_style"))?;
    Ok(Some(ResolvedProvider {
        api_style: style,
        base_url: r.get("base_url"),
        model: r.get("model"),
        key,
    }))
}

async fn exists(pool: &SqlitePool, id: i64) -> Result<bool> {
    Ok(sqlx::query("SELECT 1 FROM ai_providers WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .is_some())
}
