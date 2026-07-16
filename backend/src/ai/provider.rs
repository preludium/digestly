//! `ai_providers` persistence (prompt.md §2, §6). Admin-managed, global, exactly one active.
//! Keys are encrypted at rest ([`crypto`](super::crypto)) and **never** read back out to the API -
//! only decrypted in-process for a live call. The only key mutation is delete+create.

use anyhow::Result;
use serde::Serialize;
use sqlx::{Row, SqlitePool};

use super::{crypto, ApiStyle};

/// A provider row as returned to the admin UI - **never** includes the key (only `has_key`).
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
    pub provider_type: String,
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

/// Edit `name`/`model` only - never the key (prompt.md §6, §10).
pub async fn patch(
    pool: &SqlitePool,
    id: i64,
    name: Option<&str>,
    model: Option<&str>,
) -> Result<bool> {
    let exists = exists(pool, id).await?;
    if !exists {
        return Ok(false);
    }
    if let Some(name) = name {
        sqlx::query("UPDATE ai_providers SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(model) = model {
        sqlx::query("UPDATE ai_providers SET model = ? WHERE id = ?")
            .bind(model)
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(true)
}

/// Make `id` the single active provider (instance-wide).
pub async fn activate(pool: &SqlitePool, id: i64) -> Result<bool> {
    if !exists(pool, id).await? {
        return Ok(false);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE ai_providers SET is_active = 0")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE ai_providers SET is_active = 1 WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Delete a provider (rotating a key = delete + create). Also clears the video-provider slot if
/// it pointed at this provider, so the setting never dangles.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool> {
    let n = sqlx::query("DELETE FROM ai_providers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if n > 0 {
        sqlx::query("DELETE FROM app_settings WHERE key = ? AND value = ?")
            .bind(VIDEO_PROVIDER_KEY)
            .bind(id.to_string())
            .execute(pool)
            .await?;
    }
    Ok(n > 0)
}

/// The `app_settings` key holding the dedicated video-provider id (prompt.md §6a video path).
pub const VIDEO_PROVIDER_KEY: &str = "ai.video_provider_id";

/// The active provider with its key decrypted in memory (`None` if none is active).
pub async fn load_active(
    pool: &SqlitePool,
    enc_key: &[u8; 32],
) -> Result<Option<ResolvedProvider>> {
    let row = sqlx::query(
        "SELECT id, name, provider_type, api_style, base_url, model, api_key_enc
         FROM ai_providers WHERE is_active = 1 LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    resolve(row, enc_key)
}

/// A specific provider with its key decrypted in memory (for `POST .../{id}/test`).
pub async fn load_one(
    pool: &SqlitePool,
    enc_key: &[u8; 32],
    id: i64,
) -> Result<Option<ResolvedProvider>> {
    let row = sqlx::query(
        "SELECT id, name, provider_type, api_style, base_url, model, api_key_enc FROM ai_providers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    resolve(row, enc_key)
}

/// The dedicated video provider (prompt.md §6a): the provider `ai.video_provider_id` points at,
/// used for YouTube items only. `None` (feature off) when the setting is unset, the provider row
/// is gone, or the row isn't a Gemini provider - the only type whose API accepts a video URL.
pub async fn load_video_provider(
    pool: &SqlitePool,
    enc_key: &[u8; 32],
) -> Result<Option<ResolvedProvider>> {
    let Some(id) = sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(VIDEO_PROVIDER_KEY)
        .fetch_optional(pool)
        .await?
        .and_then(|r| r.get::<String, _>("value").parse::<i64>().ok())
    else {
        return Ok(None);
    };
    let Some(p) = load_one(pool, enc_key, id).await? else {
        return Ok(None);
    };
    if p.provider_type != "gemini" {
        tracing::warn!(provider_id = id, provider_type = %p.provider_type,
            "video provider setting points at a non-gemini provider - ignoring");
        return Ok(None);
    }
    Ok(Some(p))
}

fn resolve(
    row: Option<sqlx::sqlite::SqliteRow>,
    enc_key: &[u8; 32],
) -> Result<Option<ResolvedProvider>> {
    let Some(r) = row else { return Ok(None) };
    let enc: Option<Vec<u8>> = r.get("api_key_enc");
    let key = match enc {
        Some(blob) => Some(crypto::decrypt(enc_key, &blob)?),
        None => None,
    };
    let style = ApiStyle::parse(r.get::<String, _>("api_style").as_str())
        .ok_or_else(|| anyhow::anyhow!("stored provider has an invalid api_style"))?;
    Ok(Some(ResolvedProvider {
        provider_type: r.get("provider_type"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    const ENC_KEY: [u8; 32] = [7u8; 32];

    async fn make_provider(pool: &SqlitePool, provider_type: &str) -> i64 {
        create(
            pool,
            &ENC_KEY,
            NewProvider {
                name: provider_type.to_string(),
                provider_type: provider_type.to_string(),
                api_style: crate::ai::ApiStyle::OpenAi,
                base_url: "https://example.com/v1".to_string(),
                model: "m1".to_string(),
                key: Some("secret".to_string()),
            },
        )
        .await
        .unwrap()
    }

    async fn set_video_provider_setting(pool: &SqlitePool, id: i64) {
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ('ai.video_provider_id', ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(id.to_string())
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn video_provider_is_none_when_unset() {
        let pool = test_pool().await;
        make_provider(&pool, "gemini").await;
        assert!(load_video_provider(&pool, &ENC_KEY)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn video_provider_loads_a_gemini_provider_with_its_key() {
        let pool = test_pool().await;
        let id = make_provider(&pool, "gemini").await;
        set_video_provider_setting(&pool, id).await;

        let vp = load_video_provider(&pool, &ENC_KEY).await.unwrap().unwrap();
        assert_eq!(vp.provider_type, "gemini");
        assert_eq!(vp.model, "m1");
        assert_eq!(vp.key.as_deref(), Some("secret"));
    }

    #[tokio::test]
    async fn video_provider_rejects_non_gemini_types() {
        let pool = test_pool().await;
        let id = make_provider(&pool, "groq").await;
        set_video_provider_setting(&pool, id).await;
        assert!(load_video_provider(&pool, &ENC_KEY)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn video_provider_is_none_when_the_provider_was_deleted() {
        let pool = test_pool().await;
        set_video_provider_setting(&pool, 999).await;
        assert!(load_video_provider(&pool, &ENC_KEY)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn deleting_a_provider_clears_the_video_provider_setting() {
        let pool = test_pool().await;
        let id = make_provider(&pool, "gemini").await;
        set_video_provider_setting(&pool, id).await;

        delete(&pool, id).await.unwrap();
        assert!(load_video_provider(&pool, &ENC_KEY)
            .await
            .unwrap()
            .is_none());
        let row = sqlx::query("SELECT value FROM app_settings WHERE key = 'ai.video_provider_id'")
            .fetch_optional(&pool)
            .await
            .unwrap();
        assert!(row.is_none());
    }
}
