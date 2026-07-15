-- Track productized deployment progress without storing server secrets.

CREATE TABLE deployment_tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'waiting_for_server',
    target_type TEXT NOT NULL DEFAULT 'access_agent',
    target_id UUID NULL,
    title TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    current_step TEXT NOT NULL DEFAULT '',
    progress_percent INTEGER NOT NULL DEFAULT 0,
    report_token_hash TEXT NOT NULL,
    safe_metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    steps JSONB NOT NULL DEFAULT '[]'::jsonb,
    result JSONB NOT NULL DEFAULT '{}'::jsonb,
    error_summary TEXT NOT NULL DEFAULT '',
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ NULL,
    completed_at TIMESTAMPTZ NULL,
    CHECK (kind IN ('agent_install', 'platform_install')),
    CHECK (status IN ('waiting_for_server', 'running', 'succeeded', 'failed', 'canceled')),
    CHECK (progress_percent BETWEEN 0 AND 100)
);

CREATE INDEX deployment_tasks_status_updated_idx ON deployment_tasks (status, updated_at DESC);
CREATE INDEX deployment_tasks_target_idx ON deployment_tasks (target_type, target_id);
