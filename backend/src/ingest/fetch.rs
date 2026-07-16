//! HTTP fetching (prompt.md §4 step 2, §11 "HTTP"): conditional GET, manual redirect handling
//! (persist new URL on 301/308, cap loops), size caps, and status → outcome classification.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{
    ACCEPT, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LOCATION, RETRY_AFTER,
    USER_AGENT as UA_HEADER,
};
use reqwest::{redirect::Policy, Client, StatusCode};

use super::settings::{IngestSettings, USER_AGENT};
use super::url_util;

/// Max redirect hops before we declare a loop (prompt.md §11 "Cap redirect loops").
const MAX_REDIRECTS: u8 = 5;

/// Build the shared HTTP client: gzip/brotli, cookies off (reqwest default), a real UA, and
/// **manual** redirect handling so we can persist permanent redirects.
pub fn build_client() -> Client {
    Client::builder()
        .user_agent(USER_AGENT)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build reqwest client")
}

/// Conditional-GET request parameters carried from the feed row.
pub struct Conditional<'a> {
    pub etag: Option<&'a str>,
    pub last_modified: Option<&'a str>,
}

/// Successful fetch result.
pub struct Fetched {
    pub body: Vec<u8>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Set when the chain was an unbroken series of permanent (301/308) redirects - persist it
    /// as the feed's new `feed_url`.
    pub permanent_url: Option<String>,
}

pub enum FetchOutcome {
    /// 304 - nothing changed; caller just touches `last_fetch_at` + reschedules.
    NotModified,
    Fetched(Fetched),
}

/// Fetch failure, classified so `store` can decide disable-now vs backoff (prompt.md §11).
pub enum FetchError {
    /// Terminal (410 gone / 401 / 403) - disable the feed with this reason immediately.
    Disable(String),
    /// Rate limited - reschedule at least this far out (honor `Retry-After`).
    RetryAfter(i64, String),
    /// Transient (network, 404, 5xx, timeout, redirect loop, too large) - backoff.
    Transient(String),
}

impl FetchError {
    pub fn message(&self) -> &str {
        match self {
            FetchError::Disable(m) | FetchError::RetryAfter(_, m) | FetchError::Transient(m) => m,
        }
    }
}

/// Perform a conditional GET, following permanent/temporary redirects manually.
pub async fn get(
    client: &Client,
    url: &str,
    cond: &Conditional<'_>,
    cfg: &IngestSettings,
) -> Result<FetchOutcome, FetchError> {
    let mut current = url.to_string();
    let mut chain_permanent = true;
    let mut permanent_url: Option<String> = None;

    for _ in 0..=MAX_REDIRECTS {
        let mut req = client
            .get(&current)
            .timeout(Duration::from_secs(cfg.timeout_secs))
            .header(UA_HEADER, USER_AGENT)
            .header(
                ACCEPT,
                "application/rss+xml, application/atom+xml, application/json, text/xml, */*",
            );
        // Conditional headers only on the first (non-redirected) request.
        if current == url {
            if let Some(etag) = cond.etag.filter(|s| !s.is_empty()) {
                req = req.header(IF_NONE_MATCH, etag);
            }
            if let Some(lm) = cond.last_modified.filter(|s| !s.is_empty()) {
                req = req.header(IF_MODIFIED_SINCE, lm);
            }
        }

        let resp = req.send().await.map_err(|e| classify_reqwest(&e))?;
        let status = resp.status();

        if status == StatusCode::NOT_MODIFIED {
            return Ok(FetchOutcome::NotModified);
        }

        if status.is_redirection() {
            let permanent = matches!(
                status,
                StatusCode::MOVED_PERMANENTLY | StatusCode::PERMANENT_REDIRECT
            );
            let location = resp
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|loc| url_util::resolve(&current, loc))
                .ok_or_else(|| {
                    FetchError::Transient(format!("{status} without a usable Location"))
                })?;
            if !permanent {
                chain_permanent = false;
            }
            if chain_permanent {
                permanent_url = url_util::normalize_url(&location);
            }
            current = location;
            continue;
        }

        if !status.is_success() {
            return Err(classify_status(status, &resp));
        }

        // 2xx - read validators, then the (capped) body.
        let etag = header_str(&resp, ETAG);
        let last_modified = header_str(&resp, LAST_MODIFIED);
        let body = read_capped(resp, cfg.body_cap_bytes).await?;
        return Ok(FetchOutcome::Fetched(Fetched {
            body,
            etag,
            last_modified,
            permanent_url: permanent_url.filter(|p| p != url),
        }));
    }

    Err(FetchError::Transient("too many redirects".into()))
}

/// Read the response body, aborting if it exceeds the cap (giant-feed guard, prompt.md §11).
async fn read_capped(resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, FetchError> {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| FetchError::Transient(format!("body read error: {e}")))?;
        if buf.len() + chunk.len() > cap {
            return Err(FetchError::Transient(format!(
                "response exceeds {cap} byte cap"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn header_str(resp: &reqwest::Response, name: reqwest::header::HeaderName) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

fn classify_status(status: StatusCode, resp: &reqwest::Response) -> FetchError {
    match status {
        StatusCode::GONE => FetchError::Disable("410 Gone - feed removed".into()),
        StatusCode::UNAUTHORIZED => {
            FetchError::Disable("401 Unauthorized - feed requires auth".into())
        }
        StatusCode::FORBIDDEN => FetchError::Disable("403 Forbidden".into()),
        StatusCode::TOO_MANY_REQUESTS => {
            let secs = header_str(resp, RETRY_AFTER)
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(300)
                .clamp(30, 3600);
            FetchError::RetryAfter(secs, format!("429 Too Many Requests (retry after {secs}s)"))
        }
        StatusCode::NOT_FOUND => FetchError::Transient("404 Not Found".into()),
        s => FetchError::Transient(format!("HTTP {s}")),
    }
}

fn classify_reqwest(e: &reqwest::Error) -> FetchError {
    if e.is_timeout() {
        FetchError::Transient("request timed out".into())
    } else if e.is_connect() {
        FetchError::Transient("connection failed".into())
    } else {
        FetchError::Transient(format!("network error: {e}"))
    }
}
