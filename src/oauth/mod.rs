//! OAuth import helpers for YouTube + Reddit (prompt.md §3, §8, §9.7 - Stretch S4).
//!
//! A user links their Google/Reddit account **once**; Digestly stores only the encrypted refresh
//! token (per-user) and offers a repeatable **"Sync now"** that imports their subscribed channels /
//! subreddits as per-channel RSS feeds, **adding only the ones they don't already have**. Polling
//! itself always uses plain RSS/JSON afterward - the OAuth token is used only at sync time.
//!
//! Split of concerns:
//! * network (authorize URL, code↔token exchange, refresh, listing subscriptions) - provider-
//!   specific, needs live credentials, not unit-tested here;
//! * `reconcile` (add-missing into the user's subscriptions) + the subscription→feed-URL mapping -
//!   pure DB/string logic, covered by tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use serde::Deserialize;
use sqlx::{Row, SqlitePool};

use crate::ai::crypto;
use crate::config::OAuthClient;
use crate::error::ApiResult;
use crate::ingest::reddit;
use crate::ingest::settings::IngestSettings;
use crate::ingest::FeedKind;
use crate::routes::feeds;

const UA: &str = "digestly/0.1 (self-hosted feed reader)";
/// OAuth authorization state is a short-lived CSRF token tying the callback to the user.
const STATE_TTL: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Youtube,
    Reddit,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Youtube => "youtube",
            Provider::Reddit => "reddit",
        }
    }

    pub fn parse(s: &str) -> Option<Provider> {
        match s {
            "youtube" => Some(Provider::Youtube),
            "reddit" => Some(Provider::Reddit),
            _ => None,
        }
    }
}

/// Instance-level OAuth configuration resolved from env at boot.
pub struct OAuthSettings {
    pub google: Option<OAuthClient>,
    pub reddit: Option<OAuthClient>,
    /// Public origin used to build the redirect URI (from `RP_ORIGIN`).
    pub redirect_base: String,
}

impl OAuthSettings {
    pub fn client(&self, provider: Provider) -> Option<&OAuthClient> {
        match provider {
            Provider::Youtube => self.google.as_ref(),
            Provider::Reddit => self.reddit.as_ref(),
        }
    }

    pub fn is_enabled(&self, provider: Provider) -> bool {
        self.client(provider).is_some()
    }

    /// Redirect URI registered in the provider console: `{origin}/api/oauth/{provider}/callback`.
    pub fn redirect_uri(&self, provider: Provider) -> String {
        format!(
            "{}/api/oauth/{}/callback",
            self.redirect_base.trim_end_matches('/'),
            provider.as_str()
        )
    }
}

// ── CSRF state store (in-process, short-lived) ───────────────────────────────

struct PendingState {
    user_id: i64,
    provider: Provider,
    created: Instant,
}

/// Maps an opaque `state` value to the user + provider that started the flow.
#[derive(Clone, Default)]
pub struct OAuthStates(Arc<Mutex<HashMap<String, PendingState>>>);

impl OAuthStates {
    pub fn new() -> Self {
        OAuthStates(Arc::new(Mutex::new(HashMap::new())))
    }

    pub fn issue(&self, user_id: i64, provider: Provider) -> String {
        let mut bytes = [0u8; 24];
        OsRng.fill_bytes(&mut bytes);
        let state = hex::encode(bytes);
        let now = Instant::now();
        let mut map = self.0.lock().expect("oauth state store poisoned");
        map.retain(|_, p| now.duration_since(p.created) < STATE_TTL);
        map.insert(
            state.clone(),
            PendingState {
                user_id,
                provider,
                created: now,
            },
        );
        state
    }

    /// Consume a state token, returning the (user_id, provider) that started it if still valid.
    pub fn take(&self, state: &str) -> Option<(i64, Provider)> {
        let mut map = self.0.lock().expect("oauth state store poisoned");
        let pending = map.remove(state)?;
        if Instant::now().duration_since(pending.created) >= STATE_TTL {
            return None;
        }
        Some((pending.user_id, pending.provider))
    }
}

// ── Subscription → feed mapping + reconcile (testable core) ──────────────────

/// A remote subscription mapped to a Digestly feed.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteSubscription {
    pub feed_url: String,
    pub title: String,
    pub kind: FeedKind,
}

/// Map a YouTube channel id to its per-channel RSS feed (the same URL the poller uses).
pub fn youtube_subscription(channel_id: &str, title: &str) -> RemoteSubscription {
    RemoteSubscription {
        feed_url: format!("https://www.youtube.com/feeds/videos.xml?channel_id={channel_id}"),
        title: title.trim().to_string(),
        kind: FeedKind::Youtube,
    }
}

/// Map a subreddit name to its Digestly feed URL.
pub fn reddit_subscription(name: &str) -> RemoteSubscription {
    RemoteSubscription {
        feed_url: reddit::rss_url(name),
        title: format!("r/{name}"),
        kind: FeedKind::Reddit,
    }
}

/// Result of a sync: how many new feeds were added vs. already present.
#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq)]
pub struct SyncOutcome {
    pub added: usize,
    pub skipped: usize,
    pub total: usize,
}

/// Idempotently add the given remote subscriptions to the user's feeds under `category_id`,
/// skipping any they're already subscribed to. Safe to run repeatedly (the user's "Sync now"):
/// only genuinely-new channels/subreddits are added.
pub async fn reconcile(
    pool: &SqlitePool,
    cfg: &IngestSettings,
    user_id: i64,
    category_id: i64,
    subs: &[RemoteSubscription],
) -> ApiResult<SyncOutcome> {
    let mut added = 0usize;
    for s in subs {
        let inserted = feeds::subscribe_url(
            pool,
            cfg,
            user_id,
            &s.feed_url,
            s.kind,
            category_id,
            Some(&s.title),
            false,
        )
        .await?;
        if inserted {
            added += 1;
        }
    }
    Ok(SyncOutcome {
        added,
        skipped: subs.len() - added,
        total: subs.len(),
    })
}

// ── Token persistence (encrypted, per-user, write-only) ──────────────────────

/// A stored connection's public status (never includes the token).
#[derive(serde::Serialize)]
pub struct ConnectionStatus {
    pub provider: &'static str,
    /// Whether the instance has OAuth client credentials for this provider (drives visibility).
    pub configured: bool,
    /// Whether *this user* has linked their account.
    pub connected: bool,
    pub account_label: Option<String>,
    pub last_sync_at: Option<String>,
}

/// Persist (or replace) a user's refresh token for a provider, encrypted at rest.
pub async fn save_connection(
    pool: &SqlitePool,
    enc_key: &[u8; 32],
    user_id: i64,
    provider: Provider,
    refresh_token: &str,
    scope: Option<&str>,
    account_label: Option<&str>,
) -> Result<()> {
    let blob = crypto::encrypt(enc_key, refresh_token)?;
    sqlx::query(
        "INSERT INTO user_oauth (user_id, provider, refresh_token_enc, scope, account_label, connected_at)
         VALUES (?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(user_id, provider) DO UPDATE SET
            refresh_token_enc = excluded.refresh_token_enc,
            scope = excluded.scope,
            account_label = excluded.account_label,
            connected_at = datetime('now')",
    )
    .bind(user_id)
    .bind(provider.as_str())
    .bind(blob)
    .bind(scope)
    .bind(account_label)
    .execute(pool)
    .await?;
    Ok(())
}

/// Decrypt and return the user's stored refresh token for a provider, if connected.
pub async fn load_refresh_token(
    pool: &SqlitePool,
    enc_key: &[u8; 32],
    user_id: i64,
    provider: Provider,
) -> Result<Option<String>> {
    let row =
        sqlx::query("SELECT refresh_token_enc FROM user_oauth WHERE user_id = ? AND provider = ?")
            .bind(user_id)
            .bind(provider.as_str())
            .fetch_optional(pool)
            .await?;
    match row {
        Some(r) => {
            let blob: Vec<u8> = r.get("refresh_token_enc");
            Ok(Some(crypto::decrypt(enc_key, &blob)?))
        }
        None => Ok(None),
    }
}

/// Best-effort: trade a connected Reddit account's stored refresh token for a fresh access token,
/// so the scheduler can poll via Reddit's authenticated API instead of the public JSON endpoint,
/// which Reddit blocks/rate-limits aggressively when unauthenticated. Returns `None` on any
/// failure (no connection, revoked token, network error) - the caller falls back to the public
/// endpoint; never fatal.
///
/// **The connection is used instance-wide.** Feeds and items are shared across all users (see
/// `routes::items`), so subreddit polling is a single global job with no user to attribute it to;
/// it borrows the credential of the longest-connected Reddit account (lowest `user_id`, so the
/// choice is stable rather than whatever SQLite happens to return first). Only public subreddit
/// listings are ever requested with it - the same data the anonymous endpoint serves, just not
/// throttled. Users are told this in the Connected accounts UI (`ConnectedAccounts.tsx`).
pub async fn reddit_polling_token(
    pool: &SqlitePool,
    enc_key: &[u8; 32],
    http: &reqwest::Client,
    client: &OAuthClient,
) -> Option<String> {
    let row = sqlx::query(
        "SELECT user_id FROM user_oauth WHERE provider = 'reddit' ORDER BY user_id LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    let user_id: i64 = row.get("user_id");
    let refresh = load_refresh_token(pool, enc_key, user_id, Provider::Reddit)
        .await
        .ok()
        .flatten()?;
    access_token(http, Provider::Reddit, client, &refresh)
        .await
        .ok()
}

pub async fn delete_connection(pool: &SqlitePool, user_id: i64, provider: Provider) -> Result<()> {
    sqlx::query("DELETE FROM user_oauth WHERE user_id = ? AND provider = ?")
        .bind(user_id)
        .bind(provider.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn touch_synced(pool: &SqlitePool, user_id: i64, provider: Provider) -> Result<()> {
    sqlx::query(
        "UPDATE user_oauth SET last_sync_at = datetime('now') WHERE user_id = ? AND provider = ?",
    )
    .bind(user_id)
    .bind(provider.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

/// Per-provider connection status for the current user (no secrets).
pub async fn status_for(
    pool: &SqlitePool,
    settings: &OAuthSettings,
    user_id: i64,
) -> Result<Vec<ConnectionStatus>> {
    let mut out = Vec::with_capacity(2);
    for provider in [Provider::Youtube, Provider::Reddit] {
        let row = sqlx::query(
            "SELECT account_label, last_sync_at FROM user_oauth WHERE user_id = ? AND provider = ?",
        )
        .bind(user_id)
        .bind(provider.as_str())
        .fetch_optional(pool)
        .await?;
        out.push(ConnectionStatus {
            provider: provider.as_str(),
            configured: settings.is_enabled(provider),
            connected: row.is_some(),
            account_label: row.as_ref().and_then(|r| r.get("account_label")),
            last_sync_at: row.as_ref().and_then(|r| r.get("last_sync_at")),
        });
    }
    Ok(out)
}

// ── Provider network calls (need live credentials - not unit-tested here) ────

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

/// Build the provider's consent URL for the given `state` and redirect URI.
pub fn authorize_url(
    provider: Provider,
    client: &OAuthClient,
    redirect_uri: &str,
    state: &str,
) -> String {
    let enc = |s: &str| url_encode(s);
    match provider {
        Provider::Youtube => format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}",
            enc(&client.client_id),
            enc(redirect_uri),
            enc("https://www.googleapis.com/auth/youtube.readonly"),
            enc(state),
        ),
        Provider::Reddit => format!(
            "https://www.reddit.com/api/v1/authorize?client_id={}&response_type=code&state={}&redirect_uri={}&duration=permanent&scope={}",
            enc(&client.client_id),
            enc(state),
            enc(redirect_uri),
            enc("identity mysubreddits read"),
        ),
    }
}

/// Exchange an authorization `code` for tokens. Returns the refresh token (required for re-sync)
/// plus granted scope.
pub async fn exchange_code(
    http: &reqwest::Client,
    provider: Provider,
    client: &OAuthClient,
    redirect_uri: &str,
    code: &str,
) -> Result<(String, Option<String>)> {
    let token = request_token(
        http,
        provider,
        client,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
        ],
    )
    .await?;
    let refresh = token.refresh_token.ok_or_else(|| {
        anyhow!(
            "provider did not return a refresh token (was consent granted with offline access?)"
        )
    })?;
    Ok((refresh, token.scope))
}

/// Trade a stored refresh token for a fresh access token.
async fn access_token(
    http: &reqwest::Client,
    provider: Provider,
    client: &OAuthClient,
    refresh_token: &str,
) -> Result<String> {
    let token = request_token(
        http,
        provider,
        client,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ],
    )
    .await?;
    Ok(token.access_token)
}

async fn request_token(
    http: &reqwest::Client,
    provider: Provider,
    client: &OAuthClient,
    extra: &[(&str, &str)],
) -> Result<TokenResponse> {
    let url = match provider {
        Provider::Youtube => "https://oauth2.googleapis.com/token",
        Provider::Reddit => "https://www.reddit.com/api/v1/access_token",
    };
    let mut form: Vec<(&str, &str)> = extra.to_vec();
    let mut req = http.post(url).header(reqwest::header::USER_AGENT, UA);
    match provider {
        // Google takes the client credentials in the form body.
        Provider::Youtube => {
            form.push(("client_id", &client.client_id));
            form.push(("client_secret", &client.client_secret));
        }
        // Reddit uses HTTP Basic auth for the client credentials.
        Provider::Reddit => {
            req = req.basic_auth(&client.client_id, Some(&client.client_secret));
        }
    }
    let res = req
        .form(&form)
        .send()
        .await
        .context("OAuth token request failed")?;
    if !res.status().is_success() {
        return Err(anyhow!("OAuth token endpoint returned {}", res.status()));
    }
    res.json::<TokenResponse>()
        .await
        .context("could not parse OAuth token response")
}

/// Fetch the user's subscribed channels/subreddits, mapped to Digestly feeds.
pub async fn fetch_subscriptions(
    http: &reqwest::Client,
    provider: Provider,
    client: &OAuthClient,
    refresh_token: &str,
) -> Result<Vec<RemoteSubscription>> {
    let token = access_token(http, provider, client, refresh_token).await?;
    match provider {
        Provider::Youtube => fetch_youtube_subscriptions(http, &token).await,
        Provider::Reddit => fetch_reddit_subscriptions(http, &token).await,
    }
}

/// Optional human-friendly label for the linked account (best-effort; never fatal).
pub async fn fetch_account_label(
    http: &reqwest::Client,
    provider: Provider,
    client: &OAuthClient,
    refresh_token: &str,
) -> Option<String> {
    let token = access_token(http, provider, client, refresh_token)
        .await
        .ok()?;
    match provider {
        Provider::Reddit => {
            #[derive(Deserialize)]
            struct Me {
                name: String,
            }
            let me: Me = http
                .get("https://oauth.reddit.com/api/v1/me")
                .bearer_auth(&token)
                .header(reqwest::header::USER_AGENT, UA)
                .send()
                .await
                .ok()?
                .json()
                .await
                .ok()?;
            Some(format!("u/{}", me.name))
        }
        Provider::Youtube => Some("YouTube".to_string()),
    }
}

async fn fetch_youtube_subscriptions(
    http: &reqwest::Client,
    token: &str,
) -> Result<Vec<RemoteSubscription>> {
    #[derive(Deserialize)]
    struct Page {
        items: Vec<Item>,
        #[serde(rename = "nextPageToken")]
        next_page_token: Option<String>,
    }
    #[derive(Deserialize)]
    struct Item {
        snippet: Snippet,
    }
    #[derive(Deserialize)]
    struct Snippet {
        title: String,
        #[serde(rename = "resourceId")]
        resource_id: ResourceId,
    }
    #[derive(Deserialize)]
    struct ResourceId {
        #[serde(rename = "channelId")]
        channel_id: String,
    }

    let mut out = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
        let mut req = http
            .get("https://www.googleapis.com/youtube/v3/subscriptions")
            .bearer_auth(token)
            .query(&[("part", "snippet"), ("mine", "true"), ("maxResults", "50")]);
        if let Some(pt) = &page_token {
            req = req.query(&[("pageToken", pt.as_str())]);
        }
        let res = req
            .send()
            .await
            .context("YouTube subscriptions request failed")?;
        if !res.status().is_success() {
            return Err(anyhow!("YouTube subscriptions returned {}", res.status()));
        }
        let page: Page = res
            .json()
            .await
            .context("could not parse YouTube subscriptions")?;
        for item in page.items {
            out.push(youtube_subscription(
                &item.snippet.resource_id.channel_id,
                &item.snippet.title,
            ));
        }
        match page.next_page_token {
            Some(t) => page_token = Some(t),
            None => break,
        }
    }
    Ok(out)
}

async fn fetch_reddit_subscriptions(
    http: &reqwest::Client,
    token: &str,
) -> Result<Vec<RemoteSubscription>> {
    #[derive(Deserialize)]
    struct Listing {
        data: ListingData,
    }
    #[derive(Deserialize)]
    struct ListingData {
        children: Vec<Child>,
        after: Option<String>,
    }
    #[derive(Deserialize)]
    struct Child {
        data: SubData,
    }
    #[derive(Deserialize)]
    struct SubData {
        display_name: String,
    }

    let mut out = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let mut req = http
            .get("https://oauth.reddit.com/subreddits/mine/subscriber")
            .bearer_auth(token)
            .header(reqwest::header::USER_AGENT, UA)
            .query(&[("limit", "100")]);
        if let Some(a) = &after {
            req = req.query(&[("after", a.as_str())]);
        }
        let res = req
            .send()
            .await
            .context("Reddit subscriptions request failed")?;
        if !res.status().is_success() {
            return Err(anyhow!("Reddit subscriptions returned {}", res.status()));
        }
        let listing: Listing = res
            .json()
            .await
            .context("could not parse Reddit subscriptions")?;
        for child in listing.data.children {
            out.push(reddit_subscription(&child.data.display_name));
        }
        match listing.data.after {
            Some(a) => after = Some(a),
            None => break,
        }
    }
    Ok(out)
}

/// Minimal `application/x-www-form-urlencoded` component encoder for building authorize URLs.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_and_reddit_map_to_the_polled_feed_urls() {
        let yt = youtube_subscription("UC_x5XG1OV2P6uZZ5FSM9Ttw", "Google Developers");
        assert_eq!(
            yt.feed_url,
            "https://www.youtube.com/feeds/videos.xml?channel_id=UC_x5XG1OV2P6uZZ5FSM9Ttw"
        );
        assert_eq!(yt.kind, FeedKind::Youtube);

        let rd = reddit_subscription("rust");
        assert_eq!(rd.title, "r/rust");
        assert_eq!(rd.kind, FeedKind::Reddit);
        assert_eq!(rd.feed_url, reddit::rss_url("rust"));
    }

    #[test]
    fn authorize_urls_carry_state_and_offline_access() {
        let client = OAuthClient {
            client_id: "cid".into(),
            client_secret: "sec".into(),
        };
        let yt = authorize_url(
            Provider::Youtube,
            &client,
            "https://h.example/api/oauth/youtube/callback",
            "st8",
        );
        assert!(yt.contains("access_type=offline"));
        assert!(yt.contains("state=st8"));
        assert!(yt.contains("client_id=cid"));
        assert!(yt.contains("youtube.readonly"));
        // The secret must never appear in the front-channel URL.
        assert!(!yt.contains("sec"));

        let rd = authorize_url(
            Provider::Reddit,
            &client,
            "https://h.example/api/oauth/reddit/callback",
            "st8",
        );
        assert!(rd.contains("duration=permanent"));
        assert!(rd.contains("mysubreddits"));
        assert!(!rd.contains("sec"));
    }

    #[test]
    fn oauth_state_is_single_use_and_scoped() {
        let states = OAuthStates::new();
        let s = states.issue(7, Provider::Reddit);
        assert_eq!(states.take(&s), Some((7, Provider::Reddit)));
        assert_eq!(states.take(&s), None, "state is single-use");
        assert_eq!(states.take("bogus"), None);
    }
}
