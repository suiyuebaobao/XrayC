ALTER TABLE access_line_metric_snapshots
    ADD COLUMN IF NOT EXISTS unique_client_ips INTEGER NOT NULL DEFAULT 0;
