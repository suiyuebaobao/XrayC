-- 为入口和运行线路记录实际承载内核，节点本身保持双内核运行。

ALTER TABLE access_nodes
    ADD COLUMN IF NOT EXISTS core_type TEXT;

UPDATE access_nodes
SET core_type = 'xray'
WHERE core_type IS NULL OR btrim(core_type) = '';

ALTER TABLE access_nodes
    ALTER COLUMN core_type SET DEFAULT 'xray',
    ALTER COLUMN core_type SET NOT NULL;

ALTER TABLE access_nodes
    DROP CONSTRAINT IF EXISTS access_nodes_core_type_check;

ALTER TABLE access_nodes
    ADD CONSTRAINT access_nodes_core_type_check
    CHECK (core_type IN ('xray', 'sing_box'));

ALTER TABLE access_entries
    ADD COLUMN IF NOT EXISTS runtime_core TEXT;

UPDATE access_entries
SET runtime_core = 'xray'
WHERE runtime_core IS NULL OR btrim(runtime_core) = '';

ALTER TABLE access_entries
    ALTER COLUMN runtime_core SET DEFAULT 'xray',
    ALTER COLUMN runtime_core SET NOT NULL;

ALTER TABLE access_entries
    DROP CONSTRAINT IF EXISTS access_entries_runtime_core_check;

ALTER TABLE access_entries
    ADD CONSTRAINT access_entries_runtime_core_check
    CHECK (runtime_core IN ('xray', 'sing_box'));

ALTER TABLE access_lines
    ADD COLUMN IF NOT EXISTS runtime_core TEXT;

UPDATE access_lines al
SET runtime_core = COALESCE(e.runtime_core, 'xray')
FROM access_entry_exit_bindings b
LEFT JOIN access_entries e ON e.id = b.access_entry_id
WHERE al.id = b.id
  AND (al.runtime_core IS NULL OR btrim(al.runtime_core) = '');

UPDATE access_lines
SET runtime_core = 'xray'
WHERE runtime_core IS NULL OR btrim(runtime_core) = '';

ALTER TABLE access_lines
    ALTER COLUMN runtime_core SET DEFAULT 'xray',
    ALTER COLUMN runtime_core SET NOT NULL;

ALTER TABLE access_lines
    DROP CONSTRAINT IF EXISTS access_lines_runtime_core_check;

ALTER TABLE access_lines
    ADD CONSTRAINT access_lines_runtime_core_check
    CHECK (runtime_core IN ('xray', 'sing_box'));
