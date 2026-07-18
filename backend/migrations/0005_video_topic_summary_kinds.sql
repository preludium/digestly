CREATE TABLE item_summaries_new (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id      INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    provider_id  INTEGER NOT NULL,
    summary_kind TEXT    NOT NULL CHECK (
        summary_kind IN (
            'text',
            'video',
            'video-topics-v1',
            'text-video-topics-v1'
        )
    ),
    model        TEXT    NOT NULL,
    api_style    TEXT    NOT NULL CHECK (api_style IN ('openai', 'anthropic')),
    summary_text TEXT    NOT NULL,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (item_id, provider_id, summary_kind)
);

INSERT INTO item_summaries_new (
    id, item_id, provider_id, summary_kind, model, api_style, summary_text, created_at
)
SELECT
    id, item_id, provider_id, summary_kind, model, api_style, summary_text, created_at
FROM item_summaries;

DROP TABLE item_summaries;
ALTER TABLE item_summaries_new RENAME TO item_summaries;
CREATE INDEX idx_item_summaries_item
    ON item_summaries(item_id, provider_id, summary_kind);
