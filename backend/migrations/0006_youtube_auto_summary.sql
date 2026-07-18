ALTER TABLE items ADD COLUMN auto_summary_pending INTEGER NOT NULL DEFAULT 0;

CREATE INDEX idx_items_auto_summary_pending
    ON items(auto_summary_pending)
    WHERE auto_summary_pending = 1;

INSERT INTO app_settings (key, value)
VALUES ('ai.youtube_auto_summary_enabled', 'false')
ON CONFLICT(key) DO NOTHING;
