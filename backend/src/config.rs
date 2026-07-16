//! Startup configuration (prompt.md §8, §11, §12).
//!
//! Env vars are bootstrap-only. `SECRET_KEY` and `ADMIN_PASSWORD` are REQUIRED and the
//! process fails fast with a clear message if either is missing/blank.

use std::env;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

/// Validated runtime configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// Directory holding the SQLite file (default `/data`).
    pub data_dir: PathBuf,
    /// Address the HTTP server binds to (default `0.0.0.0:8080`).
    pub bind_addr: String,
    /// Directory of built frontend assets served via `ServeDir`.
    pub static_dir: PathBuf,
    /// Master secret: encrypts provider/ntfy secrets and signs sessions/tokens. REQUIRED.
    pub secret_key: String,
    /// Bootstraps the built-in `admin` account. REQUIRED.
    pub admin_password: String,
    /// WebAuthn Relying Party ID - the registrable domain passkeys are bound to (§1a, S1).
    /// Defaults to `localhost` (browsers allow WebAuthn over http on localhost for dev). In
    /// production set it to the stable Tailscale hostname; **changing it invalidates every
    /// existing passkey** (they are cryptographically bound to the RP ID).
    pub rp_id: String,
    /// WebAuthn Relying Party origin - the full scheme+host+port the app is served from, and the
    /// origin browsers will report during a ceremony. Must be an HTTPS origin in production (the
    /// Tailscale origin); defaults to `http://localhost:8080` for local dev.
    pub rp_origin: String,
    /// Optional, comma-separated extra WebAuthn origins to also accept alongside `rp_origin`
    /// (e.g. `http://localhost:5173` for the Vite dev server). Leave unset in production - only
    /// `rp_origin` should be trusted there.
    pub rp_extra_origins: Vec<String>,
    /// OAuth import helpers (§3, §8, S4). Optional, instance-level client credentials. A provider's
    /// import feature is shown only when its id+secret are both set. The redirect URI is derived
    /// from `rp_origin` (`{rp_origin}/api/oauth/{provider}/callback`) and must be registered in the
    /// provider's console. Refresh tokens are stored per-user, encrypted (see `user_oauth`).
    pub google_oauth: Option<OAuthClient>,
    pub reddit_oauth: Option<OAuthClient>,
}

/// An OAuth client's instance-level credentials (client id + secret).
#[derive(Clone, Debug)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: String,
}

impl Config {
    /// Load and validate from the environment. Returns a clear error on missing required vars.
    pub fn from_env() -> Result<Self> {
        let secret_key = require_env("SECRET_KEY")?;
        if secret_key.len() < 16 {
            bail!("SECRET_KEY must be at least 16 characters (it encrypts secrets and signs sessions)");
        }

        let admin_password = require_env("ADMIN_PASSWORD")?;

        let data_dir = PathBuf::from(env::var("DATA_DIR").unwrap_or_else(|_| "/data".to_string()));
        let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
        let static_dir =
            PathBuf::from(env::var("STATIC_DIR").unwrap_or_else(|_| "../web/dist".to_string()));

        let rp_id = env::var("RP_ID").unwrap_or_else(|_| "localhost".to_string());
        let rp_origin =
            env::var("RP_ORIGIN").unwrap_or_else(|_| "http://localhost:8080".to_string());
        let rp_extra_origins = parse_extra_origins();

        let google_oauth = oauth_client("GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET");
        let reddit_oauth = oauth_client("REDDIT_OAUTH_CLIENT_ID", "REDDIT_OAUTH_CLIENT_SECRET");

        Ok(Self {
            data_dir,
            bind_addr,
            static_dir,
            secret_key,
            admin_password,
            rp_id,
            rp_origin,
            rp_extra_origins,
            google_oauth,
            reddit_oauth,
        })
    }

    /// Absolute path to the SQLite database file.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("digestly.db")
    }
}

/// Build an `OAuthClient` from a pair of env vars, or `None` if either is missing/blank (the
/// feature stays hidden until both are configured).
fn oauth_client(id_var: &str, secret_var: &str) -> Option<OAuthClient> {
    let id = env::var(id_var).ok().filter(|v| !v.trim().is_empty())?;
    let secret = env::var(secret_var).ok().filter(|v| !v.trim().is_empty())?;
    Some(OAuthClient {
        client_id: id,
        client_secret: secret,
    })
}

/// `RP_EXTRA_ORIGINS` - optional, comma-separated extra WebAuthn origins to accept alongside
/// `RP_ORIGIN` (e.g. `http://localhost:5173` for the Vite dev server). Production should leave
/// this unset; only `RP_ORIGIN` should be trusted there.
fn parse_extra_origins() -> Vec<String> {
    parse_extra_origins_str(&env::var("RP_EXTRA_ORIGINS").unwrap_or_default())
}

/// Pure parsing logic, separated from env access so tests don't have to mutate process-global
/// env vars (which races under the parallel test runner).
fn parse_extra_origins_str(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Read a required env var, failing fast with a clear message if missing or blank.
fn require_env(name: &str) -> Result<String> {
    let val = env::var(name)
        .with_context(|| format!("required environment variable `{name}` is not set"))?;
    if val.trim().is_empty() {
        bail!("required environment variable `{name}` is set but empty");
    }
    Ok(val)
}

#[cfg(test)]
mod rp_extra_origins_tests {
    use super::*;

    #[test]
    fn parses_comma_separated_extra_origins_trimming_blanks() {
        let parsed = parse_extra_origins_str(" http://localhost:5173 , http://localhost:4173,, ");
        assert_eq!(
            parsed,
            vec![
                "http://localhost:5173".to_string(),
                "http://localhost:4173".to_string()
            ]
        );
    }

    #[test]
    fn defaults_to_empty_when_unset() {
        assert!(parse_extra_origins_str("").is_empty());
    }
}
