ALTER TABLE access_lines
    ADD COLUMN IF NOT EXISTS udp_packet_encoding TEXT NOT NULL DEFAULT '';

UPDATE access_lines
SET udp_packet_encoding = lower(btrim(udp_packet_encoding));

UPDATE access_lines
SET udp_packet_encoding = ''
WHERE udp_packet_encoding NOT IN ('', 'xudp');

UPDATE access_lines
SET udp_packet_encoding = 'xudp'
WHERE udp_enabled = TRUE
  AND lower(btrim(protocol)) = 'vless';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_access_lines_udp_packet_encoding'
    ) THEN
        ALTER TABLE access_lines
            ADD CONSTRAINT chk_access_lines_udp_packet_encoding
            CHECK (udp_packet_encoding IN ('', 'xudp'));
    END IF;
END $$;
