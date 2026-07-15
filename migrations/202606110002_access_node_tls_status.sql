ALTER TABLE access_nodes
  ADD COLUMN IF NOT EXISTS tls_certificates JSONB NOT NULL DEFAULT '[]'::jsonb,
  ADD COLUMN IF NOT EXISTS tls_cert_last_report_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS tls_renew_request_id UUID,
  ADD COLUMN IF NOT EXISTS tls_renew_requested_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS tls_renew_completed_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS tls_renew_status TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS tls_renew_message TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_access_nodes_tls_renew_pending
  ON access_nodes (tls_renew_requested_at DESC)
  WHERE tls_renew_request_id IS NOT NULL
    AND tls_renew_completed_at IS NULL;
