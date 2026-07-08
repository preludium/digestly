//! Pluggable AI provider layer (prompt.md §6, §6a). Provider-agnostic, admin-global: the admin
//! configures providers and picks the active one; summaries land in the shared `item_summaries`
//! cache keyed by (item, model) and are reused for every user (no duplicate token spend).
//!
//! Exactly **two** API styles exist — `openai` (OpenAI-compatible `/chat/completions`, covers
//! Groq/OpenAI/Gemini/Mistral/Ollama/custom) and `anthropic` (`/messages`). No provider-specific
//! code beyond the two [`LlmClient`] implementations in [`client`].

pub mod budget;
pub mod client;
pub mod crypto;
pub mod provider;
pub mod summarize;
pub mod transcript;

use axum::async_trait;
use serde::Serialize;

/// The two supported API styles (matches the `api_style` CHECK constraint, prompt.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiStyle {
    OpenAi,
    Anthropic,
}

impl ApiStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            ApiStyle::OpenAi => "openai",
            ApiStyle::Anthropic => "anthropic",
        }
    }

    pub fn parse(s: &str) -> Option<ApiStyle> {
        match s {
            "openai" => Some(ApiStyle::OpenAi),
            "anthropic" => Some(ApiStyle::Anthropic),
            _ => None,
        }
    }
}

/// Global AI generation parameters (prompt.md §6 "Config per provider"), stored in `app_settings`.
#[derive(Debug, Clone)]
pub struct AiParams {
    pub max_tokens: u32,
    pub temperature: f32,
    pub timeout_secs: u64,
    /// Daily/monthly token budget guard; `0` = unlimited.
    pub daily_token_budget: i64,
    pub monthly_token_budget: i64,
}

impl Default for AiParams {
    fn default() -> Self {
        Self {
            max_tokens: 1024,
            temperature: 0.3,
            timeout_secs: 60,
            daily_token_budget: 0,
            monthly_token_budget: 0,
        }
    }
}

impl AiParams {
    /// Load from `app_settings`, falling back to defaults for any unset key.
    pub async fn load(pool: &sqlx::SqlitePool) -> Self {
        let d = AiParams::default();
        Self {
            max_tokens: get_int(pool, "ai.max_tokens", d.max_tokens as i64).await.clamp(64, 8192) as u32,
            temperature: (get_int(pool, "ai.temperature_x100", (d.temperature * 100.0) as i64).await
                .clamp(0, 200) as f32)
                / 100.0,
            timeout_secs: get_int(pool, "ai.timeout_secs", d.timeout_secs as i64).await.clamp(5, 300) as u64,
            daily_token_budget: get_int(pool, "ai.daily_token_budget", d.daily_token_budget).await.max(0),
            monthly_token_budget: get_int(pool, "ai.monthly_token_budget", d.monthly_token_budget).await.max(0),
        }
    }
}

async fn get_int(pool: &sqlx::SqlitePool, key: &str, default: i64) -> i64 {
    use sqlx::Row;
    sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.get::<String, _>("value").parse().ok())
        .unwrap_or(default)
}

/// A single LLM completion request (one system + one user turn is all summarization needs).
pub struct LlmRequest {
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

/// A completion result plus best-effort token accounting for the budget guard.
pub struct LlmResponse {
    pub text: String,
    pub tokens_used: i64,
}

/// Provider call failure, with a user-safe message (never contains the key).
#[derive(Debug)]
pub enum LlmError {
    /// Network/timeout — transient.
    Network(String),
    /// Non-2xx from the provider (body captured, but keys are never in a request echo).
    Api { status: u16, message: String },
    /// 2xx but no usable text came back.
    Empty,
}

impl LlmError {
    /// A clear, safe message for on-demand callers (prompt.md §6 "clear error for on-demand").
    pub fn user_message(&self) -> String {
        match self {
            LlmError::Network(m) => format!("Could not reach the AI provider: {m}"),
            LlmError::Api { status, message } => {
                format!("AI provider returned an error ({status}): {message}")
            }
            LlmError::Empty => "AI provider returned an empty response".to_string(),
        }
    }
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_message())
    }
}

/// One concrete LLM backend. Two implementations only (openai, anthropic) — see [`client`].
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, LlmError>;
}

/// A predefined provider template (prompt.md §6). Endpoint + style baked in; the admin supplies a
/// key (and optionally overrides the model). Exposed via `GET /api/ai/presets` so the UI never
/// hardcodes them.
#[derive(Debug, Clone, Serialize)]
pub struct Preset {
    pub provider_type: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub api_style: &'static str,
    pub default_model: &'static str,
    pub needs_key: bool,
}

/// The built-in provider presets. `model` is per-provider config the admin can change, so these
/// are sensible current defaults, not hardcoded-forever ids (prompt.md §11 "AI").
pub fn presets() -> Vec<Preset> {
    vec![
        Preset {
            provider_type: "groq",
            name: "Groq",
            base_url: "https://api.groq.com/openai/v1",
            api_style: "openai",
            default_model: "llama-3.3-70b-versatile",
            needs_key: true,
        },
        Preset {
            provider_type: "openai",
            name: "OpenAI",
            base_url: "https://api.openai.com/v1",
            api_style: "openai",
            default_model: "gpt-4o-mini",
            needs_key: true,
        },
        Preset {
            provider_type: "anthropic",
            name: "Anthropic (Claude)",
            base_url: "https://api.anthropic.com/v1",
            api_style: "anthropic",
            default_model: "claude-3-5-haiku-latest",
            needs_key: true,
        },
        Preset {
            provider_type: "gemini",
            name: "Google Gemini",
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
            api_style: "openai",
            default_model: "gemini-2.0-flash",
            needs_key: true,
        },
        Preset {
            provider_type: "mistral",
            name: "Mistral",
            base_url: "https://api.mistral.ai/v1",
            api_style: "openai",
            default_model: "mistral-small-latest",
            needs_key: true,
        },
        Preset {
            provider_type: "ollama",
            name: "Ollama (local)",
            base_url: "http://localhost:11434/v1",
            api_style: "openai",
            default_model: "llama3.2",
            needs_key: false,
        },
    ]
}
