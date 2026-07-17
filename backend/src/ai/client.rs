//! The two - and only two - LLM client implementations (prompt.md §6): `OpenAICompatibleClient`
//! (`POST {base_url}/chat/completions`) and `AnthropicClient` (`POST {base_url}/messages`),
//! selected by `api_style`. Retries with backoff on 429/5xx. Keys go in headers only, never logged.

use std::time::Duration;

use axum::async_trait;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

use super::{ApiStyle, LlmError, LlmRequest, LlmResponse};

/// Max attempts (1 initial + 2 retries) on 429/5xx (prompt.md §6 "Retry with backoff").
const MAX_ATTEMPTS: u32 = 3;

/// Construct the right client for a provider's API style.
pub fn make_client(
    http: Client,
    style: ApiStyle,
    base_url: String,
    model: String,
    api_key: Option<String>,
    timeout_secs: u64,
) -> Box<dyn super::LlmClient> {
    let cfg = ClientCfg {
        http,
        base_url: base_url.trim_end_matches('/').to_string(),
        model,
        api_key,
        timeout_secs,
    };
    match style {
        ApiStyle::OpenAi => Box::new(OpenAICompatibleClient(cfg)),
        ApiStyle::Anthropic => Box::new(AnthropicClient(cfg)),
    }
}

struct ClientCfg {
    http: Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
    timeout_secs: u64,
}

/// OpenAI-compatible chat completions (Groq/OpenAI/Gemini/Mistral/Ollama/most custom).
pub struct OpenAICompatibleClient(ClientCfg);

#[async_trait]
impl super::LlmClient for OpenAICompatibleClient {
    async fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let c = &self.0;
        let url = format!("{}/chat/completions", c.base_url);
        let body = json!({
            "model": c.model,
            "messages": [
                { "role": "system", "content": req.system },
                { "role": "user", "content": req.user },
            ],
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "stream": false,
        });

        let resp = send_with_retry(c, &url, &body, |rb| match &c.api_key {
            Some(k) => rb.bearer_auth(k),
            None => rb,
        })
        .await?;

        let text = resp
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
            .ok_or(LlmError::Empty)?;
        let tokens_used = resp
            .pointer("/usage/total_tokens")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| estimate_tokens(&req.system, &req.user, &text));
        Ok(LlmResponse { text, tokens_used })
    }
}

/// Anthropic Messages API.
pub struct AnthropicClient(ClientCfg);

#[async_trait]
impl super::LlmClient for AnthropicClient {
    async fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let c = &self.0;
        let url = format!("{}/messages", c.base_url);
        let body = json!({
            "model": c.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "system": req.system,
            "messages": [ { "role": "user", "content": req.user } ],
        });

        let resp = send_with_retry(c, &url, &body, |rb| {
            let rb = rb.header("anthropic-version", "2023-06-01");
            match &c.api_key {
                Some(k) => rb.header("x-api-key", k),
                None => rb,
            }
        })
        .await?;

        let text = resp
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
            .ok_or(LlmError::Empty)?;
        let tokens_used = {
            let input = resp.pointer("/usage/input_tokens").and_then(Value::as_i64);
            let output = resp.pointer("/usage/output_tokens").and_then(Value::as_i64);
            match (input, output) {
                (Some(i), Some(o)) => i + o,
                _ => estimate_tokens(&req.system, &req.user, &text),
            }
        };
        Ok(LlmResponse { text, tokens_used })
    }
}

/// Summarize a YouTube video by URL via Gemini's native `generateContent` endpoint. This remains
/// video-only; text summaries use the configured provider route and its standard API styles.
pub async fn gemini_video_complete(
    http: Client,
    provider: &crate::ai::provider::ResolvedProvider,
    video_url: &str,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
    timeout_secs: u64,
) -> Result<LlmResponse, LlmError> {
    let c = ClientCfg {
        http,
        base_url: provider.base_url.trim_end_matches('/').to_string(),
        model: provider.model.clone(),
        api_key: provider.key.clone(),
        timeout_secs,
    };
    let url = gemini_video_endpoint(&c.base_url, &c.model);
    // Video first, instructions second - Gemini's documented best practice for media prompts.
    let body = json!({
        "contents": [{ "parts": [
            { "file_data": { "file_uri": video_url } },
            { "text": prompt },
        ]}],
        "generationConfig": {
            "maxOutputTokens": max_tokens,
            "temperature": temperature,
            "mediaResolution": "MEDIA_RESOLUTION_LOW",
        },
    });
    let resp = send_with_retry(&c, &url, &body, |rb| match &c.api_key {
        Some(k) => rb.header("x-goog-api-key", k),
        None => rb,
    })
    .await?;
    parse_gemini_video_response(&resp, prompt)
}

/// The native `generateContent` URL for a stored Gemini provider. Providers created from the
/// preset store the OpenAI-compatible base (`…/v1beta/openai`); the native API lives one level up.
fn gemini_video_endpoint(base_url: &str, model: &str) -> String {
    let root = base_url
        .trim_end_matches('/')
        .trim_end_matches("/openai")
        .trim_end_matches('/');
    format!("{root}/models/{model}:generateContent")
}

/// Extract text + token usage from a native `generateContent` response body.
fn parse_gemini_video_response(resp: &Value, prompt: &str) -> Result<LlmResponse, LlmError> {
    let text = resp
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .ok_or(LlmError::Empty)?;
    let tokens_used = resp
        .pointer("/usageMetadata/totalTokenCount")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| estimate_tokens(prompt, "", &text));
    Ok(LlmResponse { text, tokens_used })
}

/// POST JSON with per-call auth headers, retrying on 429/5xx with linear backoff. Returns the
/// parsed JSON body on success. The request body is never logged (may reference cached content,
/// and headers carry the key).
async fn send_with_retry(
    c: &ClientCfg,
    url: &str,
    body: &Value,
    apply_auth: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
) -> Result<Value, LlmError> {
    let mut last: Option<LlmError> = None;
    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            // 0.5s, 1.5s backoff.
            tokio::time::sleep(Duration::from_millis(500 * (2 * attempt as u64 - 1))).await;
        }
        let rb = c
            .http
            .post(url)
            .timeout(Duration::from_secs(c.timeout_secs))
            .json(body);
        let rb = apply_auth(rb);

        match rb.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return resp.json::<Value>().await.map_err(|e| LlmError::Api {
                        status: status.as_u16(),
                        message: format!("invalid JSON: {e}"),
                    });
                }
                let retriable = status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
                let message = error_message(resp).await;
                last = Some(LlmError::Api {
                    status: status.as_u16(),
                    message,
                });
                if !retriable {
                    break;
                }
            }
            Err(e) => {
                let msg = if e.is_timeout() {
                    "request timed out".to_string()
                } else {
                    format!("{e}")
                };
                last = Some(LlmError::Network(msg));
            }
        }
    }
    Err(last.unwrap_or(LlmError::Empty))
}

/// Best-effort provider error text (capped), tolerant of both `{error:{message}}` and `{error}`.
async fn error_message(resp: reqwest::Response) -> String {
    let raw = resp.text().await.unwrap_or_default();
    if let Ok(v) = serde_json::from_str::<Value>(&raw) {
        if let Some(m) = v.pointer("/error/message").and_then(Value::as_str) {
            return truncate(m);
        }
        if let Some(m) = v.get("error").and_then(Value::as_str) {
            return truncate(m);
        }
    }
    truncate(if raw.is_empty() {
        "no response body"
    } else {
        &raw
    })
}

fn truncate(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 300 {
        format!("{}…", s.chars().take(300).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Rough token estimate (~4 chars/token) when a provider omits usage, so the budget guard still
/// advances (prompt.md §6 "Count tokens").
fn estimate_tokens(system: &str, user: &str, output: &str) -> i64 {
    ((system.len() + user.len() + output.len()) as i64 / 4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_native_gemini_endpoint_from_the_openai_compat_base_url() {
        assert_eq!(
            gemini_video_endpoint(
                "https://generativelanguage.googleapis.com/v1beta/openai",
                "gemini-2.0-flash"
            ),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent"
        );
        // Trailing slash and no-compat-suffix variants both resolve to the same shape.
        assert_eq!(
            gemini_video_endpoint(
                "https://generativelanguage.googleapis.com/v1beta/openai/",
                "gemini-2.0-flash"
            ),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent"
        );
        assert_eq!(
            gemini_video_endpoint(
                "https://generativelanguage.googleapis.com/v1beta",
                "gemini-2.0-flash"
            ),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent"
        );
    }

    #[test]
    fn parses_a_gemini_video_response() {
        let body = serde_json::json!({
            "candidates": [{ "content": { "parts": [{ "text": "A summary." }] } }],
            "usageMetadata": { "totalTokenCount": 151234 }
        });
        let resp = parse_gemini_video_response(&body, "prompt").unwrap();
        assert_eq!(resp.text, "A summary.");
        assert_eq!(resp.tokens_used, 151234);
    }

    #[test]
    fn empty_or_missing_candidate_text_is_an_empty_error() {
        let missing = serde_json::json!({ "candidates": [] });
        assert!(matches!(
            parse_gemini_video_response(&missing, "p"),
            Err(LlmError::Empty)
        ));
        let blank = serde_json::json!({
            "candidates": [{ "content": { "parts": [{ "text": "   " }] } }]
        });
        assert!(matches!(
            parse_gemini_video_response(&blank, "p"),
            Err(LlmError::Empty)
        ));
    }

    #[test]
    fn missing_usage_falls_back_to_an_estimate() {
        let body = serde_json::json!({
            "candidates": [{ "content": { "parts": [{ "text": "Summary text here." }] } }]
        });
        let resp = parse_gemini_video_response(&body, "prompt").unwrap();
        assert!(resp.tokens_used >= 1);
    }
}
