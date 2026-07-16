//! Per-user ntfy notifications (prompt.md §7a, §11).
//!
//! Every user (admin or not) can configure a personal **ntfy** channel; Digestly only stores the
//! inputs and POSTs to it - it never bundles a server. The auth token is encrypted at rest
//! ([`crate::ai::crypto`]) and **never** returned by the API or logged. Two events fire:
//! a post-digest summary (§7) and a throttled feed-health alert (one per feed per transition,
//! de-duped so a feed shared by many users notifies each subscriber at most once).
//!
//! **SSRF:** ntfy commonly lives on localhost/LAN, so the guard here deliberately *allows* private
//! hosts (like Ollama) while still requiring a parseable http(s) URL with a host.

use std::time::Duration;

use anyhow::Result;
use reqwest::header::AUTHORIZATION;
use reqwest::Client;
use sqlx::{Row, SqlitePool};
use tracing::{debug, warn};

use crate::ai::crypto;
use crate::ingest::url_util;

/// ntfy send timeout per attempt.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// The user's ntfy config as returned to the API - **never** includes the token (only `has_token`).
#[derive(Debug, serde::Serialize)]
pub struct NotificationConfig {
    pub ntfy_server_url: Option<String>,
    pub ntfy_topic: Option<String>,
    pub ntfy_priority: i64,
    pub notify_on_digest: bool,
    pub notify_on_feed_health: bool,
    pub has_token: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            ntfy_server_url: None,
            ntfy_topic: None,
            ntfy_priority: 3,
            notify_on_digest: true,
            notify_on_feed_health: true,
            has_token: false,
        }
    }
}

/// A resolved channel with the token decrypted in memory, ready to POST to.
pub struct Channel {
    pub server: String,
    pub topic: String,
    pub token: Option<String>,
    pub priority: i64,
}

/// A single push (ntfy `POST {server}/{topic}`), title/priority/tags/click headers + body.
pub struct Push {
    pub title: String,
    pub message: String,
    pub tags: Vec<String>,
    pub click: Option<String>,
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Load the user's config for the API (token never included). `None` → no row yet (use defaults).
pub async fn load(pool: &SqlitePool, user_id: i64) -> Result<NotificationConfig> {
    let row = sqlx::query(
        "SELECT ntfy_server_url, ntfy_topic, (ntfy_auth_token_enc IS NOT NULL) AS has_token,
                ntfy_priority, notify_on_digest, notify_on_feed_health
         FROM user_notifications WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(match row {
        None => NotificationConfig::default(),
        Some(r) => NotificationConfig {
            ntfy_server_url: r.get("ntfy_server_url"),
            ntfy_topic: r.get("ntfy_topic"),
            ntfy_priority: r.get("ntfy_priority"),
            notify_on_digest: r.get::<i64, _>("notify_on_digest") != 0,
            notify_on_feed_health: r.get::<i64, _>("notify_on_feed_health") != 0,
            has_token: r.get::<i64, _>("has_token") != 0,
        },
    })
}

/// How the PUT should treat the write-only auth token.
pub enum TokenUpdate {
    /// Leave the stored token as-is.
    Keep,
    /// Clear any stored token.
    Clear,
    /// Replace with this (encrypted at rest).
    Set(String),
}

/// Upsert the user's ntfy config. Server URL is validated (http(s) + host) but private hosts are
/// allowed (ntfy is often local). The token is write-only.
#[allow(clippy::too_many_arguments)]
pub async fn save(
    pool: &SqlitePool,
    enc_key: &[u8; 32],
    user_id: i64,
    server_url: Option<&str>,
    topic: Option<&str>,
    priority: i64,
    notify_on_digest: bool,
    notify_on_feed_health: bool,
    token: TokenUpdate,
) -> Result<(), String> {
    let server = normalize_optional(server_url);
    if let Some(s) = &server {
        validate_server_url(s)?;
    }
    let topic = normalize_optional(topic).map(|t| t.trim().to_string());
    let priority = priority.clamp(1, 5);

    // Encrypt a new token if one was provided.
    let new_token_enc: Option<Vec<u8>> = match &token {
        TokenUpdate::Set(t) if !t.trim().is_empty() => Some(
            crypto::encrypt(enc_key, t.trim())
                .map_err(|_| "could not store the auth token".to_string())?,
        ),
        _ => None,
    };

    // Upsert. The token column is only touched when Set/Clear (Keep preserves the existing value).
    let token_sql = match token {
        TokenUpdate::Keep => "ntfy_auth_token_enc = ntfy_auth_token_enc",
        TokenUpdate::Clear => "ntfy_auth_token_enc = NULL",
        TokenUpdate::Set(_) => "ntfy_auth_token_enc = ?",
    };
    let sql = format!(
        "INSERT INTO user_notifications
            (user_id, ntfy_server_url, ntfy_topic, ntfy_auth_token_enc, ntfy_priority,
             notify_on_digest, notify_on_feed_health)
         VALUES (?, ?, ?, {init_token}, ?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET
             ntfy_server_url = excluded.ntfy_server_url,
             ntfy_topic = excluded.ntfy_topic,
             ntfy_priority = excluded.ntfy_priority,
             notify_on_digest = excluded.notify_on_digest,
             notify_on_feed_health = excluded.notify_on_feed_health,
             {token_sql}",
        init_token = if matches!(token, TokenUpdate::Set(_)) {
            "?"
        } else {
            "NULL"
        },
    );

    // Bind order: user_id, server, topic, [set-token-for-insert], priority, digest, health,
    // [set-token-for-update]. The token blob is bound twice when Set (insert value + update value).
    let mut q = sqlx::query(&sql).bind(user_id).bind(&server).bind(&topic);
    if let TokenUpdate::Set(_) = token {
        q = q.bind(new_token_enc.clone());
    }
    q = q
        .bind(priority)
        .bind(notify_on_digest as i64)
        .bind(notify_on_feed_health as i64);
    if let TokenUpdate::Set(_) = token {
        q = q.bind(new_token_enc);
    }
    q.execute(pool).await.map_err(|e| {
        warn!(error = %e, "failed to save notification config");
        "could not save notification settings".to_string()
    })?;
    Ok(())
}

/// Resolve the user's channel with its token decrypted. `None` when no usable channel is
/// configured (missing server or topic).
pub async fn resolve_channel(
    pool: &SqlitePool,
    enc_key: &[u8; 32],
    user_id: i64,
) -> Result<Option<Channel>> {
    let row = sqlx::query(
        "SELECT ntfy_server_url, ntfy_topic, ntfy_auth_token_enc, ntfy_priority
         FROM user_notifications WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else { return Ok(None) };

    let server: Option<String> = r.get("ntfy_server_url");
    let topic: Option<String> = r.get("ntfy_topic");
    let (Some(server), Some(topic)) = (
        server.filter(|s| !s.trim().is_empty()),
        topic.filter(|t| !t.trim().is_empty()),
    ) else {
        return Ok(None);
    };

    let token = match r.get::<Option<Vec<u8>>, _>("ntfy_auth_token_enc") {
        Some(blob) => crypto::decrypt(enc_key, &blob).ok(),
        None => None,
    };
    Ok(Some(Channel {
        server,
        topic,
        token,
        priority: r.get("ntfy_priority"),
    }))
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

/// Send one push to a channel: `POST {server}/{topic}` with headers. Timeout + one retry; a
/// failure returns a user-safe message and never crashes the caller (§7a, §11).
pub async fn send(http: &Client, ch: &Channel, push: &Push) -> Result<(), String> {
    let url = format!(
        "{}/{}",
        ch.server.trim_end_matches('/'),
        ch.topic.trim_start_matches('/')
    );
    let tags = push.tags.join(",");

    let mut last_err = String::from("send failed");
    for attempt in 0..2 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        let mut req = http
            .post(&url)
            .timeout(SEND_TIMEOUT)
            .header("Title", ascii_header(&push.title))
            .header("Priority", ch.priority.clamp(1, 5).to_string())
            .body(push.message.clone());
        if !tags.is_empty() {
            req = req.header("Tags", ascii_header(&tags));
        }
        if let Some(click) = &push.click {
            req = req.header("Click", ascii_header(click));
        }
        if let Some(token) = &ch.token {
            // The token may be a full "Bearer …"/"Basic …" header or a bare bearer token.
            let value = if token.starts_with("Bearer ") || token.starts_with("Basic ") {
                token.clone()
            } else {
                format!("Bearer {token}")
            };
            req = req.header(AUTHORIZATION, value);
        }

        match req.send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            Ok(resp) => {
                let status = resp.status();
                last_err = format!("ntfy responded {status}");
                // 4xx (bad topic/auth) won't fix on retry.
                if status.is_client_error() {
                    break;
                }
            }
            Err(e) => {
                last_err = if e.is_timeout() {
                    "ntfy request timed out".into()
                } else {
                    "could not reach the ntfy server".into()
                };
            }
        }
    }
    Err(last_err)
}

/// Send a test push to the user's channel (§7a "Test button"). Never echoes the token.
pub async fn test(
    pool: &SqlitePool,
    http: &Client,
    enc_key: &[u8; 32],
    user_id: i64,
) -> Result<(), String> {
    let ch = resolve_channel(pool, enc_key, user_id)
        .await
        .map_err(|_| "could not read your notification settings".to_string())?
        .ok_or_else(|| "configure an ntfy server URL and topic first".to_string())?;
    let push = Push {
        title: "Digestly test notification".into(),
        message: "Your Digestly ntfy channel is working. 🎉".into(),
        tags: vec!["white_check_mark".into()],
        click: None,
    };
    send(http, &ch, &push).await
}

// ---------------------------------------------------------------------------
// Feed-health event (throttled, de-duped per subscriber)
// ---------------------------------------------------------------------------

/// Distinct user ids that should be told a subscribed feed went unhealthy: they subscribe to
/// `feed_id`, have `notify_on_feed_health` on, and a server+topic configured. `DISTINCT` guards the
/// de-dupe requirement (a feed shared by many users notifies each **once**, §7a/§11).
pub async fn feed_health_recipients(pool: &SqlitePool, feed_id: i64) -> Result<Vec<i64>> {
    let rows = sqlx::query(
        "SELECT DISTINCT s.user_id AS user_id
         FROM subscriptions s
         JOIN user_notifications n ON n.user_id = s.user_id
         WHERE s.feed_id = ?
           AND n.notify_on_feed_health = 1
           AND n.ntfy_server_url IS NOT NULL AND n.ntfy_server_url <> ''
           AND n.ntfy_topic IS NOT NULL AND n.ntfy_topic <> ''",
    )
    .bind(feed_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| r.get::<i64, _>("user_id")).collect())
}

/// Fire the throttled feed-health alert to every subscriber (best-effort; per-user, no cross-user
/// leakage - each is told only about a feed they themselves subscribe to). Call once per
/// healthy→failing/disabled transition (see [`crate::ingest::store::record_failure`]).
pub async fn notify_feed_health(
    pool: &SqlitePool,
    http: &Client,
    enc_key: &[u8; 32],
    feed_id: i64,
) {
    let title: Option<String> = match sqlx::query(
        "SELECT COALESCE(NULLIF(title, ''), feed_url) AS name FROM feeds WHERE id = ?",
    )
    .bind(feed_id)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(r)) => Some(r.get("name")),
        _ => None,
    };
    let feed_name = title.unwrap_or_else(|| "A feed".into());

    let recipients = match feed_health_recipients(pool, feed_id).await {
        Ok(r) => r,
        Err(e) => {
            warn!(feed_id, error = %e, "could not load feed-health recipients");
            return;
        }
    };
    if recipients.is_empty() {
        return;
    }
    debug!(
        feed_id,
        subscribers = recipients.len(),
        "sending feed-health notifications"
    );

    for user_id in recipients {
        let ch = match resolve_channel(pool, enc_key, user_id).await {
            Ok(Some(ch)) => ch,
            _ => continue,
        };
        let push = Push {
            title: "Digestly: feed problem".into(),
            message: format!("“{feed_name}” is having trouble fetching and may be paused."),
            tags: vec!["warning".into()],
            click: None,
        };
        if let Err(e) = send(http, &ch, &push).await {
            // Surfaced via logs; never crashes ingestion (§7a).
            warn!(feed_id, user_id, error = %e, "feed-health notification failed");
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn normalize_optional(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Validate a ntfy server URL: must be a parseable http(s) URL with a host. Private/loopback hosts
/// are **allowed** (ntfy is commonly on localhost/LAN, §7a "SSRF note").
pub fn validate_server_url(url: &str) -> Result<(), String> {
    match url_util::normalize_url(url) {
        Some(n) if !url_util::host_of(&n).is_empty() => Ok(()),
        _ => Err("ntfy server URL must be a valid http(s) address".to_string()),
    }
}

/// Header-safe rendering of a possibly-unicode string: keep printable ASCII, replace the rest so a
/// title with emoji/accents can't produce an invalid header (the body stays full UTF-8).
fn ascii_header(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                ' '
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Digestly".to_string()
    } else {
        trimmed.chars().take(200).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_server_url_allows_local_and_public_rejects_junk() {
        assert!(validate_server_url("https://ntfy.sh").is_ok());
        assert!(
            validate_server_url("http://localhost:8080").is_ok(),
            "ntfy on localhost is allowed"
        );
        assert!(
            validate_server_url("http://192.168.1.5").is_ok(),
            "LAN ntfy is allowed"
        );
        assert!(validate_server_url("not a url").is_err());
        assert!(validate_server_url("ftp://example.com").is_err());
    }

    #[test]
    fn ascii_header_strips_non_ascii() {
        assert_eq!(ascii_header("Hello"), "Hello");
        assert_eq!(ascii_header("Café ☕"), "Caf");
        assert_eq!(ascii_header("☕☕"), "Digestly");
    }
}
