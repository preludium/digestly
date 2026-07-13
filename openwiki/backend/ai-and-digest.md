# AI & digest engine

AI is provider-agnostic and admin-global. The admin configures one or more providers and picks the active one; every user's summaries/digests use that provider.

**Source:** `src/ai/` (client, provider, crypto, summarize, transcript, transcript_worker, budget), `src/digest/` (mod, cron)

## Pluggable AI providers

Two API styles exist, with exactly two `LlmClient` implementations (`src/ai/client.rs`):

| Style | Implementation | Endpoint |
|-------|---------------|----------|
| `openai` | `OpenAICompatibleClient` | `POST {base_url}/chat/completions` |
| `anthropic` | `AnthropicClient` | `POST {base_url}/messages` |

The `openai` style covers Groq, OpenAI, Gemini (OpenAI-compatible), Mistral, Ollama, and most custom endpoints. No provider-specific code beyond these two clients.

### Presets (`src/ai/provider.rs`)

Predefined providers bake in base URL + API style. The admin supplies only a key (except Ollama, no key) and model. Exposed via `GET /api/ai/presets`:
- Groq (`https://api.groq.com/openai/v1`, openai)
- OpenAI (`https://api.openai.com/v1`, openai)
- Anthropic (anthropic)
- Google Gemini (`https://generativelanguage.googleapis.com/v1beta/openai`, openai)
- Mistral (`https://api.mistral.ai/v1`, openai)
- Ollama (`http://localhost:11434/v1`, openai, no key)

Custom endpoints allow arbitrary `name`, `base_url`, `api_style`, key, and `model`.

### Write-only keys (`src/ai/crypto.rs`)

Keys are encrypted at rest with a `SECRET_KEY`-derived ChaCha20-Poly1305 key (key = SHA-256 of `SECRET_KEY`, blob = `nonce(12) ‖ ciphertext+tag`). Keys are **never returned** by any endpoint or logged. Rotation is delete + recreate.

### SSRF guard

Custom base URLs are validated to reject private/loopback ranges unless `allow-private` is enabled. Intentionally **allows localhost for Ollama** (`provider_type == ollama`).

### Token budget guard (`src/ai/budget.rs`)

Daily/monthly token budgets are checked before a call and recorded after. Huge source lists are truncated. Budgets configured in `app_settings` (`ai.daily_token_budget`, `ai.monthly_token_budget`); `0` = unlimited.

Global AI params (`src/ai/mod.rs`, `AiParams`):
- `max_tokens` (clamped 64–8192, default 1024)
- `temperature` (clamped 0.0–2.0, default 0.3)
- `timeout_secs` (clamped 5–300, default 60)
- `daily_token_budget`, `monthly_token_budget`

## Shared summary cache

Summaries are written to `item_summaries` keyed by `(item_id, model)` and reused across all users. The cache is global (no user-identifying data). The same item is never re-summarized unless the model differs or the user forces a refresh.

**Source:** `src/ai/summarize.rs`

## On-demand summarization

`POST /api/ai/summarize` (in `src/routes/ai.rs`):
- Checks the shared cache first; returns cached summary if available for that model
- Produces a structured summary via the active provider
- Stores in `item_summaries` for future reuse
- Returns a clear error to the UI on failure (nothing is cached on error)

## Video → readable (`src/ai/transcript.rs`, `src/ai/transcript_worker.rs`)

Video items are rendered as text, not players. The transcript is fetched lazily by a background worker:

- For each new YouTube video, fetches captions (prefer manual, fall back to auto-generated)
- Stores in `transcript_text`, sets `transcript_status ∈ {fetched, unavailable}`
- If no captions → `transcript_status = unavailable`, falls back to description

The reader renders: **AI summary (primary) → collapsible full transcript → de-emphasized "Watch on YouTube" link**. No embedded/autoplay player.

The transcript worker (`src/ai/transcript_worker.rs`) is a background tokio task notified by the ingestion scheduler when new YouTube items are stored. It runs independently from the main ingestion loop.

## Digest engine (`src/digest/`)

The digest engine is **global/admin-configured** (one cron schedule, look-back window, enabled, categories) but **content is per-user**: each run iterates users and builds each one a digest of _their_ subscriptions grouped _by their_ categories.

### Config (`DigestConfig` in `src/digest/mod.rs`)

Stored in `app_settings`:
- `digest.enabled` — master on/off
- `digest.cron` — restricted 5-field cron expression
- `digest.lookback_hours` — look-back window for items
- `digest.timezone` — timezone for cron matching (DST-correct)
- `digest.categories` — `"all"` or comma-separated category names
- `digest.ai_enabled` — whether to use AI summarization

### Cron parser (`src/digest/cron.rs`)

A restricted 5-field parser matched against wall-clock time in the configured timezone (DST-correct), with a `describe()` human preview. Not a full cron library — supports minute, hour, day-of-month, month, day-of-week with `*`, specific values, and ranges.

### Run algorithm (`run_all` in `src/digest/mod.rs`)

1. Resolve the active AI provider + params once
2. For each user:
   - Gather in-window items grouped by their categories (respecting `min_score`)
   - For each non-empty category, produce **one AI prompt** ("Summarize these developments in 3–4 concise bullets" + titles/sources)
   - Cap at `MAX_ITEMS_PER_CATEGORY_PROMPT` (40) items per category prompt
3. **Raw-titles fallback:** if no active provider, or a provider call fails, or budget is exceeded, that section falls back to raw grouped titles + links with a `fallback_note`. The run **never fails**.
4. Archive each digest to `digests` as `payload_json` (categories, sources, `ai_used`, `fallback_note`, `failure_warning`)
5. If user has ntfy enabled, push to their channel
6. **Failure warning:** if > 2 of a user's sources failed to fetch in the window, include a `failure_warning` in both the digest and push

### Scheduling

The digest scheduler ticks every 45s and fires at most once per matching minute (a `digest.last_run` stamp guard). It is a background tokio task spawned in `src/main.rs`.

## ntfy notifications (`src/notify/mod.rs`)

Per-user config lives in `user_notifications`:
- `ntfy_server_url`, `ntfy_topic` — where to POST
- `ntfy_auth_token_enc` — encrypted write-only bearer token
- `ntfy_priority` — ntfy priority level
- `notify_on_digest` / `notify_on_feed_health` — per-event toggles

Sending is an HTTP `POST {server}/{topic}` with Title/Priority/Tags (+ auth) headers, a 10s timeout and one retry. Failures are logged and surfaced, never fatal. The SSRF guard **allows** the user-configured ntfy host (often localhost/LAN).

Feed-health pushes are throttled to one per feed per `healthy → failing/disabled` transition and de-duped per subscriber.

## API routes

- `GET /api/ai/presets` — list predefined providers
- `GET /api/ai/providers` — list configured providers (admin, keys hidden)
- `POST /api/ai/providers` — add provider (admin)
- `DELETE /api/ai/providers/{id}` — delete provider (admin)
- `POST /api/ai/providers/{id}/activate` — set as active (admin)
- `POST /api/ai/providers/{id}/test` — test connection (admin, key never echoed)
- `POST /api/ai/summarize` — on-demand summarize (any user)
- `POST /api/digest/run` — manual trigger (admin)
- `GET /api/digest/config` / `PUT /api/digest/config` — digest config (admin)
- `GET /api/digest` — user's digest history
- `GET /api/digest/{id}` — specific digest detail
- `GET/PUT /api/notifications` — user's ntfy config
- `POST /api/notifications/test` — test ntfy delivery (user, token never echoed)
