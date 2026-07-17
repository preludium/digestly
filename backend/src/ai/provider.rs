//! `ai_providers` persistence (prompt.md §2, §6). Admin-managed, global, exactly one active.
//! Keys are encrypted at rest ([`crypto`](super::crypto)) and **never** read back out to the API -
//! only decrypted in-process for a live call. The only key mutation is delete+create.

use anyhow::Result;
use serde::Serialize;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

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
    pub is_video_only: bool,
}

/// A provider resolved for a live call, with its decrypted key held only in memory.
pub struct ResolvedProvider {
    pub id: i64,
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
    let video_provider_id = selected_video_provider_id(pool).await?;
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
            is_video_only: Some(r.get("id")) == video_provider_id,
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

/// Delete a provider (rotating a key = delete + create), clearing every routing reference.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let n = sqlx::query("DELETE FROM ai_providers WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n > 0 {
        sqlx::query("DELETE FROM app_settings WHERE key = ? AND value = ?")
            .bind(VIDEO_PROVIDER_KEY)
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
        remove_from_text_route(&mut tx, id).await?;
        tx.commit().await?;
    }
    Ok(n > 0)
}

/// The `app_settings` key holding the dedicated video-provider id (prompt.md §6a video path).
pub const VIDEO_PROVIDER_KEY: &str = "ai.video_provider_id";
pub const TEXT_PROVIDER_MODE_KEY: &str = "ai.text_provider_mode";
/// The pre-public-DTO key is read for upgrades from the routing WIP, but never written.
const LEGACY_ROUTING_MODE_KEY: &str = "ai.routing_mode";
pub const TEXT_ROUTE_PROVIDER_IDS_KEY: &str = "ai.text_route_provider_ids";

/// The configured text-provider route. New installations retain the legacy single-active-provider
/// behavior until an admin explicitly selects a single provider or switches to `ordered`.
pub async fn load_text_route(
    pool: &SqlitePool,
    enc_key: &[u8; 32],
) -> Result<Vec<ResolvedProvider>> {
    let video_id = selected_video_provider_id(pool).await?;
    let ids = text_provider_ids(pool).await?;

    let mut route = Vec::with_capacity(ids.len());
    for id in ids {
        if Some(id) == video_id {
            continue;
        }
        if let Some(provider) = load_one(pool, enc_key, id).await? {
            route.push(provider);
        }
    }
    Ok(route)
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

/// Transaction-scoped variant used when validating a provider immediately before writing a setting.
pub async fn load_one_tx(
    tx: &mut Transaction<'_, Sqlite>,
    enc_key: &[u8; 32],
    id: i64,
) -> Result<Option<ResolvedProvider>> {
    let row = sqlx::query(
        "SELECT id, name, provider_type, api_style, base_url, model, api_key_enc FROM ai_providers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
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
    let Some(id) = selected_video_provider_id(pool).await? else {
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

pub async fn selected_video_provider_id(pool: &SqlitePool) -> Result<Option<i64>> {
    Ok(setting(pool, VIDEO_PROVIDER_KEY)
        .await?
        .and_then(|value| value.parse::<i64>().ok()))
}

pub async fn selected_video_provider_id_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<Option<i64>> {
    Ok(sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(VIDEO_PROVIDER_KEY)
        .fetch_optional(&mut **tx)
        .await?
        .map(|row| row.get::<String, _>("value"))
        .and_then(|value| value.parse::<i64>().ok()))
}

/// The text providers selected by the current routing configuration. Single mode falls back to the
/// active provider only when no valid explicit selection exists.
pub async fn selected_text_provider_ids(pool: &SqlitePool) -> Result<Vec<i64>> {
    let video_id = selected_video_provider_id(pool).await?;
    Ok(text_provider_ids(pool)
        .await?
        .into_iter()
        .filter(|id| Some(*id) != video_id)
        .collect())
}

async fn text_provider_ids(pool: &SqlitePool) -> Result<Vec<i64>> {
    let mode = text_provider_mode(pool).await?;
    let configured = setting(pool, TEXT_ROUTE_PROVIDER_IDS_KEY).await?;
    if mode == "ordered" {
        return Ok(configured
            .and_then(|value| serde_json::from_str::<Vec<i64>>(&value).ok())
            .unwrap_or_default());
    }
    if let Some(ids) = configured.and_then(|value| serde_json::from_str::<Vec<i64>>(&value).ok()) {
        return Ok(ids.into_iter().take(1).collect());
    }
    Ok(
        sqlx::query("SELECT id FROM ai_providers WHERE is_active = 1 LIMIT 1")
            .fetch_optional(pool)
            .await?
            .map(|row| row.get("id"))
            .into_iter()
            .collect(),
    )
}

async fn remove_from_text_route(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: i64,
) -> Result<()> {
    let Some(value) = sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(TEXT_ROUTE_PROVIDER_IDS_KEY)
        .fetch_optional(&mut **tx)
        .await?
        .map(|row| row.get::<String, _>("value"))
    else {
        return Ok(());
    };
    let Ok(ids) = serde_json::from_str::<Vec<i64>>(&value) else {
        return Ok(());
    };
    let filtered: Vec<_> = ids
        .into_iter()
        .filter(|candidate| *candidate != id)
        .collect();
    if filtered.len() == serde_json::from_str::<Vec<i64>>(&value)?.len() {
        return Ok(());
    }
    if filtered.is_empty() && text_provider_mode_tx(tx).await? == "single" {
        sqlx::query("DELETE FROM app_settings WHERE key = ?")
            .bind(TEXT_ROUTE_PROVIDER_IDS_KEY)
            .execute(&mut **tx)
            .await?;
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)\n         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(TEXT_ROUTE_PROVIDER_IDS_KEY)
    .bind(serde_json::to_string(&filtered)?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn text_provider_mode_tx(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<String> {
    let value = sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(TEXT_PROVIDER_MODE_KEY)
        .fetch_optional(&mut **tx)
        .await?
        .map(|row| row.get::<String, _>("value"));
    let value = match value {
        Some(value) => Some(value),
        None => sqlx::query("SELECT value FROM app_settings WHERE key = ?")
            .bind(LEGACY_ROUTING_MODE_KEY)
            .fetch_optional(&mut **tx)
            .await?
            .map(|row| row.get::<String, _>("value")),
    };
    Ok(match value.as_deref() {
        Some("ordered") | Some("route") => "ordered".to_string(),
        _ => "single".to_string(),
    })
}

async fn setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    Ok(sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?
        .map(|row| row.get("value")))
}

/// Return the canonical mode, translating the short-lived internal WIP values for upgrades.
pub async fn text_provider_mode(pool: &SqlitePool) -> Result<String> {
    let value =
        setting(pool, TEXT_PROVIDER_MODE_KEY)
            .await?
            .or(setting(pool, LEGACY_ROUTING_MODE_KEY).await?);
    Ok(match value.as_deref() {
        Some("ordered") | Some("route") => "ordered".to_string(),
        _ => "single".to_string(),
    })
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
        id: r.get("id"),
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
    async fn text_route_defaults_to_the_legacy_active_provider() {
        let pool = test_pool().await;
        let id = make_provider(&pool, "groq").await;

        let route = load_text_route(&pool, &ENC_KEY).await.unwrap();
        assert_eq!(route.len(), 1);
        assert_eq!(route[0].id, id);
    }

    #[tokio::test]
    async fn single_route_uses_non_active_selection_and_excludes_video_provider() {
        let pool = test_pool().await;
        let active = make_provider(&pool, "groq").await;
        let selected = make_provider(&pool, "openai").await;
        let video = make_provider(&pool, "gemini").await;
        set_video_provider_setting(&pool, video).await;
        sqlx::query("INSERT INTO app_settings (key, value) VALUES (?, ?)")
            .bind(TEXT_ROUTE_PROVIDER_IDS_KEY)
            .bind(serde_json::json!([selected]).to_string())
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            selected_text_provider_ids(&pool).await.unwrap(),
            vec![selected]
        );
        assert_eq!(
            load_text_route(&pool, &ENC_KEY).await.unwrap()[0].id,
            selected
        );
        assert_ne!(selected, active);

        sqlx::query("UPDATE app_settings SET value = ? WHERE key = ?")
            .bind(serde_json::json!([video]).to_string())
            .bind(TEXT_ROUTE_PROVIDER_IDS_KEY)
            .execute(&pool)
            .await
            .unwrap();
        assert!(selected_text_provider_ids(&pool).await.unwrap().is_empty());
        assert!(load_text_route(&pool, &ENC_KEY).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn configured_text_route_preserves_order_and_excludes_video_provider() {
        let pool = test_pool().await;
        let first = make_provider(&pool, "groq").await;
        let video = make_provider(&pool, "gemini").await;
        let last = make_provider(&pool, "openai").await;
        set_video_provider_setting(&pool, video).await;
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, 'ordered')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(TEXT_PROVIDER_MODE_KEY)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(TEXT_ROUTE_PROVIDER_IDS_KEY)
        .bind(serde_json::json!([first, video, last]).to_string())
        .execute(&pool)
        .await
        .unwrap();

        let route = load_text_route(&pool, &ENC_KEY).await.unwrap();
        assert_eq!(
            route.iter().map(|provider| provider.id).collect::<Vec<_>>(),
            [first, last]
        );
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

    #[tokio::test]
    async fn deleting_a_provider_removes_it_from_the_ordered_route() {
        let pool = test_pool().await;
        let removed = make_provider(&pool, "groq").await;
        let kept = make_provider(&pool, "openai").await;
        sqlx::query("INSERT INTO app_settings (key, value) VALUES (?, 'ordered')")
            .bind(TEXT_PROVIDER_MODE_KEY)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO app_settings (key, value) VALUES (?, ?)")
            .bind(TEXT_ROUTE_PROVIDER_IDS_KEY)
            .bind(serde_json::json!([removed, kept]).to_string())
            .execute(&pool)
            .await
            .unwrap();

        delete(&pool, removed).await.unwrap();

        assert_eq!(selected_text_provider_ids(&pool).await.unwrap(), vec![kept]);
    }

    #[tokio::test]
    async fn deleting_the_single_selection_falls_back_to_the_active_provider() {
        let pool = test_pool().await;
        let active = make_provider(&pool, "groq").await;
        let selected = make_provider(&pool, "openai").await;
        sqlx::query("INSERT INTO app_settings (key, value) VALUES (?, ?)")
            .bind(TEXT_ROUTE_PROVIDER_IDS_KEY)
            .bind(serde_json::json!([selected]).to_string())
            .execute(&pool)
            .await
            .unwrap();

        delete(&pool, selected).await.unwrap();

        assert_eq!(
            selected_text_provider_ids(&pool).await.unwrap(),
            vec![active]
        );
    }
}
