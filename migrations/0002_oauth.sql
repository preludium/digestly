-- Per-user OAuth connections (Stretch S4). Lets a user link their YouTube / Reddit account so
-- Digestly can import (and re-sync) their subscribed channels / subreddits as per-channel RSS
-- feeds. Only a refresh token is stored, ENCRYPTED at rest with the SECRET_KEY-derived key (same
-- scheme as AI provider keys and ntfy tokens); it is never returned by any endpoint or logged.
--
-- The OAuth *client* credentials (client id/secret) are instance-level env config, not stored here.
-- Polling itself always uses plain RSS/JSON afterward — the token is only used at sync time.
CREATE TABLE user_oauth (
    user_id            INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider           TEXT    NOT NULL,                       -- 'youtube' | 'reddit'
    refresh_token_enc  BLOB    NOT NULL,                       -- encrypted refresh token (write-only)
    scope              TEXT,                                   -- granted scopes (informational)
    account_label      TEXT,                                   -- e.g. the linked account name, for display
    connected_at       TEXT    NOT NULL DEFAULT (datetime('now')),
    last_sync_at       TEXT,
    PRIMARY KEY (user_id, provider)
);
