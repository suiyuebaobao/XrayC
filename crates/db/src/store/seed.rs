//! 演示数据种子写入逻辑。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::dirty::*;
use super::existence::*;
use super::line_binding::*;
use super::probes::*;
use super::rows::*;
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl PgStore {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn reset_demo_data_for_tests(&self) -> Result<(), DbError> {
        let name = sqlx::query_scalar::<_, String>("SELECT current_database()")
            .fetch_one(&self.pool)
            .await?;
        if !(name == "test"
            || name == "ci"
            || name.starts_with("test_")
            || name.ends_with("_test")
            || name.starts_with("xrayc_test_")
            || name.starts_with("ci_")
            || name.ends_with("_ci"))
        {
            return Err(DbError::InvalidInput(
                "拒绝在非隔离测试数据库重置演示数据".to_string(),
            ));
        }
        let table_list = sqlx::query_scalar::<_, Option<String>>(
            r#"
            SELECT string_agg(format('%I.%I', schemaname, tablename), ', ')
            FROM pg_tables
            WHERE schemaname = 'public'
              AND tablename <> '_sqlx_migrations'
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        if let Some(table_list) = table_list.filter(|value| !value.trim().is_empty()) {
            let sql = format!("TRUNCATE TABLE {table_list} RESTART IDENTITY CASCADE");
            sqlx::query(&sql).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn seed_demo_data(&self) -> Result<(), DbError> {
        #[cfg(any(test, feature = "test-support"))]
        self.reset_demo_data_for_tests().await?;

        let user_id = uuid("00000000-0000-0000-0000-000000000001");
        let plan_id = uuid("00000000-0000-0000-0000-000000000101");
        let node_id = uuid("00000000-0000-0000-0000-000000000201");
        let exit_resource_id = uuid("00000000-0000-0000-0000-000000000301");
        let exit_endpoint_id = uuid("00000000-0000-0000-0000-000000000302");
        let exit_pool_id = uuid("00000000-0000-0000-0000-000000000401");
        let line_id = uuid("00000000-0000-0000-0000-000000000501");
        let group_id = uuid("00000000-0000-0000-0000-000000000601");
        let admin_id = uuid("00000000-0000-0000-0000-000000000002");
        let xray_user_key = format!("u-{}@xrayc.local", user_id.simple());
        let admin_xray_user_key = format!("u-{}@xrayc.local", admin_id.simple());
        let demo_agent_token_hash = agent_token_hash(DEMO_AGENT_TOKEN);
        let demo_user_password_hash =
            demo_password_hash(DEMO_USER_PASSWORD, b"xrayc-demo-user-salt")?;
        let demo_admin_password_hash =
            demo_password_hash(DEMO_ADMIN_PASSWORD, b"xrayc-demo-admin-salt")?;

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO users (
                id, email, password_hash, xray_user_key, access_credential, disabled,
                is_admin, display_name
            )
            VALUES ($1, 'demo@example.test', $2, $3, $4, FALSE, FALSE, '演示用户')
            ON CONFLICT (id) DO UPDATE SET
                email = EXCLUDED.email,
                password_hash = EXCLUDED.password_hash,
                xray_user_key = EXCLUDED.xray_user_key,
                access_credential = COALESCE(NULLIF(users.access_credential, ''), EXCLUDED.access_credential),
                is_admin = FALSE,
                display_name = EXCLUDED.display_name,
                disabled = FALSE
            "#,
        )
        .bind(user_id)
        .bind(&demo_user_password_hash)
        .bind(&xray_user_key)
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO users (
                id, email, password_hash, xray_user_key, access_credential, disabled,
                is_admin, display_name
            )
            VALUES ($1, 'admin@example.test', $2, $3, $4, FALSE, TRUE, 'XrayC 管理员')
            ON CONFLICT (id) DO UPDATE SET
                email = EXCLUDED.email,
                password_hash = EXCLUDED.password_hash,
                xray_user_key = EXCLUDED.xray_user_key,
                access_credential = COALESCE(NULLIF(users.access_credential, ''), EXCLUDED.access_credential),
                is_admin = TRUE,
                display_name = EXCLUDED.display_name,
                disabled = FALSE
            "#,
        )
        .bind(admin_id)
        .bind(&demo_admin_password_hash)
        .bind(admin_xray_user_key)
        .bind(admin_id.to_string())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO plans (
                id, name, is_default, traffic_limit_bytes,
                billing_multiplier, enabled
            )
            VALUES ($1, '基础套餐', TRUE, $2, 1.000, TRUE)
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                is_default = TRUE,
                traffic_limit_bytes = EXCLUDED.traffic_limit_bytes,
                enabled = TRUE
            "#,
        )
        .bind(plan_id)
        .bind(10_i64 * 1024 * 1024 * 1024)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO user_subscriptions (
                user_id, plan_id, active, expires_at, used_bytes,
                limit_bytes
            )
            VALUES ($1, $2, TRUE, $3, 0, $4)
            ON CONFLICT (user_id) DO UPDATE SET
                plan_id = EXCLUDED.plan_id,
                active = TRUE,
                expires_at = GREATEST(user_subscriptions.expires_at, EXCLUDED.expires_at),
                limit_bytes = GREATEST(user_subscriptions.limit_bytes, EXCLUDED.limit_bytes),
                updated_at = now()
            "#,
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(Utc::now() + Duration::days(30))
        .bind(10_i64 * 1024 * 1024 * 1024)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO subscription_tokens (
                token, token_hash, user_id, revoked_at, expires_at
            )
            VALUES ('demo-token', $1, $2, NULL, $3)
            ON CONFLICT (user_id) DO UPDATE SET
                token = EXCLUDED.token,
                token_hash = EXCLUDED.token_hash,
                revoked_at = NULL,
                expires_at = EXCLUDED.expires_at
            "#,
        )
        .bind(agent_token_hash("demo-token"))
        .bind(user_id)
        .bind(subscription_token_expires_at())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO access_nodes (
                id, name, public_host, agent_token_hash, config_dirty,
                desired_config_hash, applied_config_hash
            )
            VALUES ($1, '本机中转节点', 'access.example.test', $2, TRUE, 'seeded-desired', NULL)
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                public_host = CASE
                    WHEN access_nodes.public_host = ''
                        OR access_nodes.public_host ILIKE '%example%'
                        OR access_nodes.public_host IN ('0.0.0.0', '127.0.0.1', 'localhost')
                    THEN EXCLUDED.public_host
                    ELSE access_nodes.public_host
                END,
                agent_token_hash = EXCLUDED.agent_token_hash,
                config_dirty = TRUE,
                desired_config_hash = EXCLUDED.desired_config_hash
            "#,
        )
        .bind(node_id)
        .bind(&demo_agent_token_hash)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO exit_resources (
                id, name, region_code, ownership, access_node_id, enabled
            )
            VALUES ($1, 'seeded-socks-exit', 'HK', 'third_party', NULL, TRUE)
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                region_code = EXCLUDED.region_code,
                ownership = EXCLUDED.ownership,
                access_node_id = EXCLUDED.access_node_id,
                enabled = TRUE
            "#,
        )
        .bind(exit_resource_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO exit_endpoints (id, exit_resource_id, outbound_type, host, port, outbound_config, enabled)
            VALUES ($1, $2, $3::endpoint_type, '198.51.100.10', 1080, '{}'::jsonb, TRUE)
            ON CONFLICT (id) DO UPDATE SET
                exit_resource_id = EXCLUDED.exit_resource_id,
                outbound_type = EXCLUDED.outbound_type,
                host = EXCLUDED.host,
                port = EXCLUDED.port,
                outbound_config = EXCLUDED.outbound_config,
                enabled = TRUE
            "#,
        )
        .bind(exit_endpoint_id)
        .bind(exit_resource_id)
        .bind("socks")
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO exit_pools (id, name)
            VALUES ($1, '默认出口池')
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                updated_at = now()
            "#,
        )
        .bind(exit_pool_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO exit_pool_members (exit_pool_id, exit_endpoint_id, weight, status)
            VALUES ($1, $2, 100, 'healthy')
            ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
                weight = EXCLUDED.weight,
                status = EXCLUDED.status
            "#,
        )
        .bind(exit_pool_id)
        .bind(exit_endpoint_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO line_groups (id, name, country_code, icon, exit_pool_id, sort_weight, enabled)
            VALUES (
                $1, '默认分组', 'HK', '🇭🇰', $2, 100, TRUE
            )
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                country_code = EXCLUDED.country_code,
                icon = EXCLUDED.icon,
                exit_pool_id = EXCLUDED.exit_pool_id,
                sort_weight = EXCLUDED.sort_weight,
                enabled = EXCLUDED.enabled
            "#,
        )
        .bind(group_id)
        .bind(exit_pool_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO access_lines (
                id, name, access_node_id, line_group_id, exit_endpoint_id, exit_pool_id, listen_host, listen_port,
                protocol, transport, user_uuid, server_name, public_key, short_id,
                enabled, udp_packet_encoding
            )
            VALUES (
                $1, '香港 01', $2, $3, $4, $5, 'access.example.test', 443,
                'vless', 'tcp', $6, 'www.cloudflare.com', 'seeded-public-key',
                'a1b2c3d4', TRUE, ''
            )
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                access_node_id = EXCLUDED.access_node_id,
                line_group_id = EXCLUDED.line_group_id,
                exit_endpoint_id = EXCLUDED.exit_endpoint_id,
                exit_pool_id = EXCLUDED.exit_pool_id,
                listen_host = CASE
                    WHEN access_lines.listen_host = ''
                        OR access_lines.listen_host ILIKE '%example%'
                        OR access_lines.listen_host IN ('0.0.0.0', '127.0.0.1', 'localhost')
                    THEN EXCLUDED.listen_host
                    ELSE access_lines.listen_host
                END,
                listen_port = EXCLUDED.listen_port,
                protocol = EXCLUDED.protocol,
                transport = EXCLUDED.transport,
                user_uuid = EXCLUDED.user_uuid,
                server_name = EXCLUDED.server_name,
                public_key = EXCLUDED.public_key,
                short_id = EXCLUDED.short_id,
                udp_packet_encoding = EXCLUDED.udp_packet_encoding,
                enabled = TRUE
            "#,
        )
        .bind(line_id)
        .bind(node_id)
        .bind(group_id)
        .bind(exit_endpoint_id)
        .bind(exit_pool_id)
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO access_entries (
                id, access_node_id, name, listen_host, listen_port, protocol, transport,
                security, user_uuid, server_name, public_key, short_id, flow, udp_enabled,
                udp_packet_encoding, xhttp_path, xhttp_host, xhttp_mode, inbound_config,
                enabled, sort_weight
            )
            SELECT
                id, access_node_id, name, listen_host, listen_port, protocol, transport,
                COALESCE(NULLIF(inbound_config->>'security', ''), ''),
                user_uuid, server_name, public_key, short_id, flow, udp_enabled,
                udp_packet_encoding, xhttp_path, xhttp_host, xhttp_mode, inbound_config,
                enabled, 100
            FROM access_lines
            WHERE id = $1
            ON CONFLICT (id) DO UPDATE SET
                access_node_id = EXCLUDED.access_node_id,
                name = EXCLUDED.name,
                listen_host = EXCLUDED.listen_host,
                listen_port = EXCLUDED.listen_port,
                protocol = EXCLUDED.protocol,
                transport = EXCLUDED.transport,
                security = EXCLUDED.security,
                user_uuid = EXCLUDED.user_uuid,
                server_name = EXCLUDED.server_name,
                public_key = EXCLUDED.public_key,
                short_id = EXCLUDED.short_id,
                flow = EXCLUDED.flow,
                udp_enabled = EXCLUDED.udp_enabled,
                udp_packet_encoding = EXCLUDED.udp_packet_encoding,
                xhttp_path = EXCLUDED.xhttp_path,
                xhttp_host = EXCLUDED.xhttp_host,
                xhttp_mode = EXCLUDED.xhttp_mode,
                inbound_config = EXCLUDED.inbound_config,
                enabled = EXCLUDED.enabled,
                updated_at = now()
            "#,
        )
        .bind(line_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO access_entry_exit_bindings (
                id, access_entry_id, exit_endpoint_id, exit_pool_id, name, enabled, sort_weight
            )
            SELECT id, id, exit_endpoint_id, exit_pool_id, name, enabled, 100
            FROM access_lines
            WHERE id = $1 AND exit_endpoint_id IS NOT NULL
            ON CONFLICT (id) DO UPDATE SET
                access_entry_id = EXCLUDED.access_entry_id,
                exit_endpoint_id = EXCLUDED.exit_endpoint_id,
                exit_pool_id = EXCLUDED.exit_pool_id,
                name = EXCLUDED.name,
                enabled = EXCLUDED.enabled,
                updated_at = now()
            "#,
        )
        .bind(line_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO line_groups (id, name, country_code, icon, exit_pool_id, sort_weight, enabled)
            VALUES (
                $1, '默认分组', 'HK', '🇭🇰', $2, 100, TRUE
            )
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                country_code = EXCLUDED.country_code,
                icon = EXCLUDED.icon,
                exit_pool_id = EXCLUDED.exit_pool_id,
                sort_weight = EXCLUDED.sort_weight,
                enabled = EXCLUDED.enabled
            "#,
        )
        .bind(group_id)
        .bind(exit_pool_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM line_group_lines
            WHERE line_group_id = $1
            "#,
        )
        .bind(group_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
            VALUES ($1, $2)
            ON CONFLICT (line_group_id, exit_endpoint_id) DO NOTHING
            "#,
        )
        .bind(group_id)
        .bind(exit_endpoint_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO line_group_binding_nodes (
                line_group_id, entry_exit_binding_id, position
            )
            VALUES ($1, $2, 100)
            ON CONFLICT (line_group_id, entry_exit_binding_id) DO UPDATE SET
                position = EXCLUDED.position
            "#,
        )
        .bind(group_id)
        .bind(line_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO plan_line_groups (plan_id, line_group_id)
            VALUES ($1, $2)
            ON CONFLICT (plan_id, line_group_id) DO UPDATE SET
                line_group_id = EXCLUDED.line_group_id
            "#,
        )
        .bind(plan_id)
        .bind(group_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}
