-- 运行版本为状态数据，不改业务配置、日志或现有节点身份。
ALTER TABLE access_nodes ADD COLUMN xray_version TEXT NOT NULL DEFAULT '';
CREATE TABLE service_runtime_versions (
    instance_id UUID PRIMARY KEY,
    service_name TEXT NOT NULL,
    package_version TEXT NOT NULL,
    release_id TEXT NOT NULL,
    heartbeat_interval_seconds INTEGER NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
