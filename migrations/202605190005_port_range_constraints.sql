DO $$
DECLARE
    invalid_access_line_ports BIGINT;
    invalid_exit_endpoint_ports BIGINT;
BEGIN
    SELECT COUNT(*) INTO invalid_access_line_ports
    FROM access_lines
    WHERE listen_port < 1 OR listen_port > 65535;

    IF invalid_access_line_ports > 0 THEN
        RAISE EXCEPTION 'access_lines.listen_port 存在无效端口数量: %', invalid_access_line_ports;
    END IF;

    SELECT COUNT(*) INTO invalid_exit_endpoint_ports
    FROM exit_endpoints
    WHERE port < 0
       OR port > 65535
       OR (outbound_type <> 'direct' AND port = 0);

    IF invalid_exit_endpoint_ports > 0 THEN
        RAISE EXCEPTION 'exit_endpoints.port 存在无效端口数量: %', invalid_exit_endpoint_ports;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'chk_access_lines_listen_port_range'
    ) THEN
        ALTER TABLE access_lines
            ADD CONSTRAINT chk_access_lines_listen_port_range
            CHECK (listen_port BETWEEN 1 AND 65535);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'chk_exit_endpoints_port_range'
    ) THEN
        ALTER TABLE exit_endpoints
            ADD CONSTRAINT chk_exit_endpoints_port_range
            CHECK (
                port BETWEEN 0 AND 65535
                AND (outbound_type = 'direct' OR port > 0)
            );
    END IF;
END $$;
