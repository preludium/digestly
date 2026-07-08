-- Digestly full schema (prompt.md §2). Global Rule #3: the ENTIRE schema lands in
-- Phase 1 even for columns later phases use. Later phases add data/logic, not columns.
--
-- Model: "shared ingest, per-user state". Global tables have no user_id (feeds/items/AI);
-- per-user tables all carry user_id and cascade-delete with the user.
--
-- Timestamps are stored as TEXT ISO-8601 UTC (SQLite datetime('now') => UTC). Rendered in
-- the user's timezone by the app. Booleans are INTEGER 0/1.

PRAGMA foreign_keys = ON;

-- ============================================================================
-- Accounts & auth (global)
-- ============================================================================

CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT,                                  -- argon2; NULL only if passkey-only (stretch)
    role          TEXT    NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    disabled      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    last_login_at TEXT
);

CREATE TABLE passkeys (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id TEXT    NOT NULL UNIQUE,
    public_key    BLOB    NOT NULL,
    sign_count    INTEGER NOT NULL DEFAULT 0,
    name          TEXT,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    last_used_at  TEXT
);
CREATE INDEX idx_passkeys_user ON passkeys(user_id);

-- Sessions: revocable on logout / user-delete. (Stateless signed tokens also reference this
-- for revocation.) Cascade with the user.
CREATE TABLE sessions (
    id         TEXT    PRIMARY KEY,                       -- opaque session id (signed into the cookie)
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT    NOT NULL
);
CREATE INDEX idx_sessions_user ON sessions(user_id);

-- ============================================================================
-- Global content (fetched once, shared by all users — NOT user-scoped)
-- ============================================================================

CREATE TABLE feeds (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_url           TEXT    NOT NULL UNIQUE,           -- normalized
    site_url           TEXT,
    title              TEXT,
    description        TEXT,
    icon_url           TEXT,
    kind               TEXT    NOT NULL DEFAULT 'rss'
                         CHECK (kind IN ('rss', 'atom', 'jsonfeed', 'youtube', 'reddit')),
    etag               TEXT,
    last_modified      TEXT,
    last_fetch_at      TEXT,
    next_fetch_at      TEXT,
    fetch_interval_secs INTEGER NOT NULL DEFAULT 3600,
    failure_count      INTEGER NOT NULL DEFAULT 0,
    last_error         TEXT,
    disabled           INTEGER NOT NULL DEFAULT 0,
    created_at         TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_feeds_next_fetch ON feeds(next_fetch_at);

CREATE TABLE items (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_id           INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    guid              TEXT,
    url               TEXT,
    title             TEXT,
    author            TEXT,
    content_html      TEXT,                               -- sanitized (ammonia)
    content_text      TEXT,                               -- stripped, for FTS + AI
    transcript_text   TEXT,
    transcript_status TEXT    NOT NULL DEFAULT 'none'
                        CHECK (transcript_status IN ('none', 'fetched', 'unavailable')),
    image_url         TEXT,
    duration_secs     INTEGER,
    reading_time_secs INTEGER,
    published_at      TEXT,
    fetched_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    score             INTEGER,
    comments_count    INTEGER,
    upvote_ratio      REAL,
    dedup_hash        TEXT
);
CREATE INDEX idx_items_feed_published ON items(feed_id, published_at);
CREATE INDEX idx_items_dedup          ON items(dedup_hash);
CREATE INDEX idx_items_published      ON items(published_at);
CREATE INDEX idx_items_score          ON items(score);
CREATE INDEX idx_items_comments       ON items(comments_count);

-- FTS5 over title/content_text/author, external-content mirror of items.
CREATE VIRTUAL TABLE items_fts USING fts5(
    title,
    content_text,
    author,
    content = 'items',
    content_rowid = 'id'
);

-- Keep the FTS index in sync with items (safe + forward-compatible; later phases just insert rows).
CREATE TRIGGER items_ai AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, title, content_text, author)
    VALUES (new.id, new.title, new.content_text, new.author);
END;
CREATE TRIGGER items_ad AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, title, content_text, author)
    VALUES ('delete', old.id, old.title, old.content_text, old.author);
END;
CREATE TRIGGER items_au AFTER UPDATE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, title, content_text, author)
    VALUES ('delete', old.id, old.title, old.content_text, old.author);
    INSERT INTO items_fts(rowid, title, content_text, author)
    VALUES (new.id, new.title, new.content_text, new.author);
END;

-- Shared AI summary cache keyed by (item, model). No user-identifying data.
CREATE TABLE item_summaries (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id      INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    model        TEXT    NOT NULL,
    api_style    TEXT    NOT NULL CHECK (api_style IN ('openai', 'anthropic')),
    summary_text TEXT    NOT NULL,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (item_id, model)
);
CREATE INDEX idx_item_summaries_item ON item_summaries(item_id, model);

-- Global, admin-managed LLM endpoints. api_key_enc encrypted at rest, NEVER returned.
CREATE TABLE ai_providers (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT    NOT NULL,
    provider_type TEXT    NOT NULL,
    api_style     TEXT    NOT NULL CHECK (api_style IN ('openai', 'anthropic')),
    base_url      TEXT    NOT NULL,
    model         TEXT    NOT NULL,
    api_key_enc   BLOB,                                   -- encrypted; NULL for keyless (Ollama)
    is_active     INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- Global, admin-only config (allow_registration, ingestion tunables, digest engine, AI params).
CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- ============================================================================
-- Per-user state (every row has user_id; cascade-deleted with the user)
-- ============================================================================

CREATE TABLE categories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    position   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, name)
);
CREATE INDEX idx_categories_user ON categories(user_id);

CREATE TABLE subscriptions (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id          INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    feed_id          INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    category_id      INTEGER NOT NULL REFERENCES categories(id),
    content_type     TEXT    NOT NULL DEFAULT 'reading'
                       CHECK (content_type IN ('reading', 'video')),
    min_score        INTEGER NOT NULL DEFAULT 0,
    full_text_extract INTEGER NOT NULL DEFAULT 0,
    disabled         INTEGER NOT NULL DEFAULT 0,
    title_override   TEXT,
    created_at       TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, feed_id)
);
CREATE INDEX idx_subscriptions_user      ON subscriptions(user_id);
CREATE INDEX idx_subscriptions_feed      ON subscriptions(feed_id);
CREATE INDEX idx_subscriptions_user_cat  ON subscriptions(user_id, category_id);

CREATE TABLE item_states (
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    item_id   INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    is_read   INTEGER NOT NULL DEFAULT 0,
    is_starred INTEGER NOT NULL DEFAULT 0,
    read_at   TEXT,
    PRIMARY KEY (user_id, item_id)
);
CREATE INDEX idx_item_states_user_item ON item_states(user_id, item_id);

CREATE TABLE settings (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    key     TEXT    NOT NULL,
    value   TEXT    NOT NULL,
    PRIMARY KEY (user_id, key)
);

CREATE TABLE user_notifications (
    user_id             INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    ntfy_server_url     TEXT,
    ntfy_topic          TEXT,
    ntfy_auth_token_enc BLOB,                             -- encrypted; never returned
    ntfy_priority       INTEGER NOT NULL DEFAULT 3,
    notify_on_digest    INTEGER NOT NULL DEFAULT 1,
    notify_on_feed_health INTEGER NOT NULL DEFAULT 1,
    created_at          TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE digests (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    period_start TEXT    NOT NULL,
    period_end   TEXT    NOT NULL,
    item_count   INTEGER NOT NULL DEFAULT 0,
    payload_json TEXT,
    notified     INTEGER NOT NULL DEFAULT 0,
    error        TEXT
);
CREATE INDEX idx_digests_user ON digests(user_id, created_at);
