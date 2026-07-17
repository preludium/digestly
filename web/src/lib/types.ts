// Shared API types mirroring the Rust DTOs (prompt.md §10). Keep in sync with `src/routes`.

export type Role = "admin" | "user";

export interface User {
    id: number;
    username: string;
    role: Role;
}

export interface AdminUser {
    id: number;
    username: string;
    role: Role;
    disabled: boolean;
    created_at: string;
    last_login_at: string | null;
    subscription_count: number;
}

export interface Category {
    id: number;
    name: string;
    position: number;
    feed_count: number;
    deletable: boolean;
}

export type FeedKind = "rss" | "atom" | "jsonfeed" | "youtube" | "reddit";
export type ContentType = "reading" | "video";

/** A user's subscription over a shared feed (the UI's "feed"). Mirrors Rust `FeedDto`. */
export interface Feed {
    id: number;
    feed_id: number;
    feed_url: string;
    title: string;
    kind: FeedKind;
    site_url: string | null;
    icon_url: string | null;
    category_id: number;
    category_name: string;
    content_type: ContentType;
    min_score: number;
    full_text_extract: boolean;
    fetch_interval_secs: number;
    disabled: boolean;
    item_count: number;
    last_fetch_at: string | null;
    last_error: string | null;
    failure_count: number;
    feed_disabled: boolean;
}

export interface DiscoverCandidate {
    feed_url: string;
    title: string | null;
    kind: FeedKind;
    site_url: string | null;
    icon_url: string | null;
    already_subscribed: boolean;
}

export type FeedStatus = "ok" | "failing" | "disabled";

export interface FeedHealth {
    id: number;
    feed_id: number;
    title: string;
    feed_url: string;
    kind: FeedKind;
    status: FeedStatus;
    last_fetch_at: string | null;
    next_fetch_at: string | null;
    failure_count: number;
    last_error: string | null;
}

export interface SubscribeInput {
    feed_url: string;
    kind: FeedKind;
    title?: string | null;
    site_url?: string | null;
    category_id: number;
    content_type?: ContentType;
    min_score?: number;
    full_text_extract?: boolean;
    title_override?: string | null;
    fetch_interval_secs?: number;
}

export type TranscriptStatus = "none" | "fetched" | "unavailable";

/** A card/grid item (prompt.md §10 `GET /api/items`). State fields are the current user's. */
export interface Item {
    id: number;
    feed_id: number;
    category: string;
    feed_title: string;
    kind: FeedKind;
    content_type: ContentType;
    title: string | null;
    url: string | null;
    author: string | null;
    snippet: string | null;
    image_url: string | null;
    published_at: string | null;
    is_read: boolean;
    is_starred: boolean;
    reading_time_secs: number | null;
    duration_secs: number | null;
    score: number | null;
    comments_count: number | null;
    upvote_ratio: number | null;
    transcript_status: TranscriptStatus;
    has_summary: boolean;
    site_url: string | null;
    feed_icon_url: string | null;
}

/** Full item for the preview surface (prompt.md §9.1a, `GET /api/items/{id}`). */
export interface ItemDetail extends Item {
    content_html: string | null;
    transcript_text: string | null;
    summary: string | null;
}

export interface ItemsPage {
    items: Item[];
    page: number;
    page_size: number;
    total_pages: number;
    total_count: number;
}

export interface CategoryCount {
    category_id: number;
    count: number;
}

export interface CategoryCounts {
    total: number;
    categories: CategoryCount[];
}

export interface ItemState {
    is_read: boolean;
    is_starred: boolean;
}

export type ItemType = "all" | "reading" | "video";
export type ItemStatus = "all" | "unread" | "starred";
export type ItemWhen = "all" | "24h" | "today" | "yesterday" | "week" | "month";
export type ItemSort = "new" | "old" | "quick" | "top" | "discussed" | "unread";

export interface AdminSettings {
    allow_registration: boolean;
}

// --- AI (Phase 5) ---
export type ApiStyle = "openai" | "anthropic";

/** A predefined provider template (prompt.md §6, `GET /api/ai/presets`). */
export interface AiPreset {
    provider_type: string;
    name: string;
    base_url: string;
    api_style: ApiStyle;
    default_model: string;
    needs_key: boolean;
}

/** A configured provider as listed to the admin - never includes the key (only `has_key`). */
export interface AiProvider {
    id: number;
    name: string;
    provider_type: string;
    api_style: ApiStyle;
    base_url: string;
    model: string;
    has_key: boolean;
    is_active: boolean;
    is_video_only: boolean;
}

export interface NewAiProvider {
    name: string;
    provider_type: string;
    api_style: ApiStyle;
    base_url: string;
    model: string;
    key?: string;
}

export type TextProviderMode = "single" | "ordered";

/** Global AI generation params + current usage (prompt.md §6, §9.7). */
export interface AiSettings {
    max_tokens: number;
    temperature: number;
    timeout_secs: number;
    daily_token_budget: number;
    monthly_token_budget: number;
    tokens_used_today: number;
    tokens_used_month: number;
    text_provider_mode: TextProviderMode;
    text_provider_ids: number[];
    video_provider_id: number | null;
}

export type AiSettingsInput = Partial<
    Omit<AiSettings, "tokens_used_today" | "tokens_used_month">
>;

export interface SummaryResult {
    summary: string;
    model: string;
    cached: boolean;
}

export interface TestResult {
    ok: boolean;
    error?: string;
}

export interface RegistrationStatus {
    allow_registration: boolean;
    /** Whether the server has passkeys/WebAuthn enabled (RP configured). Drives the login button. */
    passkeys_enabled: boolean;
}

/** A registered passkey (prompt.md §9.12). Public-key material is never sent to the client. */
export interface Passkey {
    id: number;
    name: string;
    created_at: string;
    last_used_at: string | null;
}

// --- OAuth import connections (YouTube / Reddit - S4) ---
export type OAuthProvider = "youtube" | "reddit";

/** Per-provider connection status (prompt.md §3, §9.7). Never includes tokens. */
export interface OAuthConnection {
    provider: OAuthProvider;
    /** The server has client credentials for this provider (feature is shown). */
    configured: boolean;
    /** This user has linked their account. */
    connected: boolean;
    account_label: string | null;
    last_sync_at: string | null;
}

/** Result of a "Sync now": feeds added vs. already present. */
export interface SyncOutcome {
    added: number;
    skipped: number;
    total: number;
}

// --- Notifications (ntfy, per-user - Phase 6) ---
/** The current user's ntfy config (prompt.md §7a, §10). Never includes the token (only `has_token`). */
export interface NotificationConfig {
    ntfy_server_url: string | null;
    ntfy_topic: string | null;
    ntfy_priority: number;
    notify_on_digest: boolean;
    notify_on_feed_health: boolean;
    has_token: boolean;
}

/** PUT body: token is write-only (omit = keep, "" = clear, value = set). */
export interface PutNotifications {
    ntfy_server_url?: string | null;
    ntfy_topic?: string | null;
    ntfy_priority?: number;
    notify_on_digest?: boolean;
    notify_on_feed_health?: boolean;
    auth_token?: string;
}

// --- Digest (Phase 6) ---
/** Digest engine config (admin-only, prompt.md §7, §9.7). `categories: null` = all. */
export interface DigestConfig {
    enabled: boolean;
    cron: string;
    lookback_days: number;
    timezone: string;
    categories: string[] | null;
    ai_enabled: boolean;
    schedule_preview: string;
}

export type PutDigestConfig = Omit<DigestConfig, "schedule_preview">;

/** Read-only global digest schedule, available to every authenticated user. */
export interface DigestSchedule {
    enabled: boolean;
    description: string;
    timezone: string;
    next_run_at: string | null;
}

export interface DigestListItem {
    id: number;
    created_at: string;
    period_start: string;
    period_end: string;
    item_count: number;
    notified: boolean;
    error: string | null;
}

export interface DigestCategorySection {
    name: string;
    ai_summary: string | null;
    raw: boolean;
    items: {
        title: string;
        url: string | null;
        feed_title: string;
        published_at: string | null;
    }[];
}

export interface DigestPayload {
    generated_at: string;
    period_start: string;
    period_end: string;
    ai_used: boolean;
    fallback_note: string | null;
    failed_sources: number;
    failure_warning: string | null;
    sources: string[];
    categories: DigestCategorySection[];
}

export interface DigestDetailData extends DigestListItem {
    payload: DigestPayload | null;
}

export interface DigestRunSummary {
    users: number;
    digests: number;
    pushed: number;
}

// --- Preferences + OPML + ingestion (Phase 7) ---
export type Density = "normal" | "compact";

/** Per-user preferences (prompt.md §8, §9.7 General, `GET/PUT /api/settings`). */
export interface UserSettings {
    sort: ItemSort;
    content_view: ItemType;
    page_size: number;
    timezone: string;
    density: Density;
    auto_mark_read: boolean;
    theme: "light" | "dark";
    onboarded: boolean;
}

export interface OpmlPreviewEntry {
    feed_url: string;
    title: string | null;
    kind: FeedKind;
    category: string | null;
    already_subscribed: boolean;
}

export interface OpmlImportItem {
    feed_url: string;
    title: string | null;
    kind: FeedKind;
    category: string | null;
}

export interface OpmlImportResult {
    imported: number;
    skipped: number;
}

/** Ingestion + retention tunables (admin-only, `GET/PUT /api/admin/ingestion`). */
export interface IngestionSettings {
    concurrency: number;
    per_host_delay_ms: number;
    timeout_secs: number;
    default_interval_secs: number;
    allow_private: boolean;
    max_item_age_days: number;
    retention_max_age_days: number;
    retention_max_per_feed: number;
}

/** The caller's in-flight "Ingest now" run + cooldown (`GET /api/ingest/status`). */
export interface IngestStatus {
    run: { run_id: number; done: number; total: number } | null;
    cooldown_secs: number;
}

/** Pushed over the SSE stream (`GET /api/events`). Discriminated on `type`. */
export type ServerEvent =
    | { type: "feed_polled"; run_id: number; done: number; total: number }
    | {
          type: "ingest_finished";
          run_id: number;
          new_items: number;
          failed: number;
          timed_out: boolean;
      };
