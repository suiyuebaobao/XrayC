ALTER TABLE line_groups
    ADD COLUMN IF NOT EXISTS country_code TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS icon TEXT NOT NULL DEFAULT '';

UPDATE line_groups
SET country_code = 'GLOBAL'
WHERE btrim(country_code) = '';

UPDATE line_groups
SET icon = '🌐'
WHERE btrim(icon) = '';
