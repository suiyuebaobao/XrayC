-- 收紧出口资源归属约束，避免普通/第三方出口伪装成本机 direct。
-- 历史数据不做破坏性删除；约束使用 NOT VALID，后续新写入必须满足规则。

UPDATE exit_resources r
SET ownership = 'local_direct',
    access_node_id = COALESCE(
        r.access_node_id,
        (SELECT id FROM access_nodes ORDER BY created_at ASC LIMIT 1)
    )
WHERE r.name = 'local-direct'
  AND EXISTS (SELECT 1 FROM access_nodes)
  AND EXISTS (
      SELECT 1
      FROM exit_endpoints e
      WHERE e.exit_resource_id = r.id
        AND e.outbound_type = 'direct'
  );

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_exit_resources_ownership'
    ) THEN
        ALTER TABLE exit_resources
            ADD CONSTRAINT chk_exit_resources_ownership
            CHECK (ownership IN ('third_party', 'self_hosted', 'local_direct')) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_exit_resources_local_direct_owner'
    ) THEN
        ALTER TABLE exit_resources
            ADD CONSTRAINT chk_exit_resources_local_direct_owner
            CHECK (
                (ownership = 'local_direct' AND access_node_id IS NOT NULL)
                OR (ownership <> 'local_direct' AND access_node_id IS NULL)
            ) NOT VALID;
    END IF;
END $$;
