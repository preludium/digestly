-- A model name alone is not a provider identity: two configured providers may use the same model,
-- and native-video output must not satisfy a text-summary cache lookup.
CREATE TABLE item_summaries_new (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id      INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    provider_id  INTEGER NOT NULL,
    summary_kind TEXT    NOT NULL CHECK (summary_kind IN ('text', 'video')),
    model        TEXT    NOT NULL,
    api_style    TEXT    NOT NULL CHECK (api_style IN ('openai', 'anthropic')),
    summary_text TEXT    NOT NULL,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (item_id, provider_id, summary_kind)
);

-- Reuse a legacy row only for the active provider with the same model and API style. Legacy rows
-- have no provider provenance, so assigning them to every matching provider would invent cache
-- hits. Rows without an unambiguous active-provider match retain their contents under a distinct
-- negative sentinel ID and will be regenerated.
INSERT INTO item_summaries_new (item_id, provider_id, summary_kind, model, api_style, summary_text, created_at)
SELECT s.item_id, p.id, 'text', s.model, s.api_style, s.summary_text, s.created_at
FROM item_summaries s
JOIN ai_providers p ON p.id = (
    SELECT candidate.id
    FROM ai_providers candidate
    WHERE candidate.model = s.model
      AND candidate.api_style = s.api_style
      AND candidate.is_active = 1
    ORDER BY candidate.id
    LIMIT 1
);

INSERT INTO item_summaries_new (item_id, provider_id, summary_kind, model, api_style, summary_text, created_at)
SELECT s.item_id, -s.id, 'text', s.model, s.api_style, s.summary_text, s.created_at
FROM item_summaries s
WHERE NOT EXISTS (
    SELECT 1
    FROM ai_providers p
    WHERE p.model = s.model AND p.api_style = s.api_style AND p.is_active = 1
);

DROP TABLE item_summaries;
ALTER TABLE item_summaries_new RENAME TO item_summaries;
CREATE INDEX idx_item_summaries_item ON item_summaries(item_id, provider_id, summary_kind);
