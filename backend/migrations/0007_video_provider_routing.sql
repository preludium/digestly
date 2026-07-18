-- Preserve the prior dedicated slot as an explicit single-provider route.
INSERT INTO app_settings (key, value)
SELECT 'ai.video_provider_mode', 'single'
WHERE EXISTS (
    SELECT 1 FROM app_settings WHERE key = 'ai.video_provider_id'
)
ON CONFLICT(key) DO NOTHING;

INSERT INTO app_settings (key, value)
SELECT 'ai.video_provider_ids', json_array(CAST(value AS INTEGER))
FROM app_settings
WHERE key = 'ai.video_provider_id'
ON CONFLICT(key) DO NOTHING;

DELETE FROM app_settings WHERE key = 'ai.video_provider_id';
