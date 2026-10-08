-- 配置修订采用单调计数，不依赖时间戳分辨率判断并发变更。
ALTER TABLE access_nodes ADD COLUMN config_revision BIGINT NOT NULL DEFAULT 0;
ALTER TABLE access_nodes ADD COLUMN config_built_revision BIGINT NOT NULL DEFAULT -1;
CREATE FUNCTION xrayc_bump_config_revision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.config_dirty AND NEW.config_dirty_at IS NOT NULL THEN
        NEW.config_revision := OLD.config_revision + 1;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_access_node_config_revision BEFORE UPDATE OF config_dirty_at ON access_nodes
FOR EACH ROW EXECUTE FUNCTION xrayc_bump_config_revision();

-- 全局运行策略与节点失效标记在同一事务中提交，防止等待中的配置写入被缓存遗漏。
CREATE FUNCTION xrayc_invalidate_runtime_policy() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE changed_key TEXT;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF NEW.setting_value IS NOT DISTINCT FROM OLD.setting_value THEN RETURN NULL; END IF;
    END IF;
    IF TG_OP='DELETE' THEN changed_key:=OLD.setting_key; ELSE changed_key:=NEW.setting_key; END IF;
    IF changed_key IN ('subscription_config','access_operations') THEN
        UPDATE access_nodes SET config_dirty=TRUE, config_dirty_at=clock_timestamp(),
            desired_config_hash=NULL, config_dirty_reason='runtime_policy_changed';
    END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER trg_runtime_policy_revision AFTER INSERT OR UPDATE OR DELETE ON site_settings
FOR EACH ROW EXECUTE FUNCTION xrayc_invalidate_runtime_policy();
