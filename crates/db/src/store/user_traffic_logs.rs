//! 管理员用户级流量日志查询。
//! 本模块只提供按用户读取 usage_ledgers 的只读接口。
//! 会话 IP 来自 access_user_session_events，并按账本时间就近关联。
//! 查询结果包含中转入口、接入节点和出口端点摘要。
//! 管理员界面需要完整展示用户维度记录，因此这里不做字段脱敏。
//! 不在本模块写入账本、会话或任何运行态数据。
//! 分页和筛选在 SQL 层完成，避免前端拉取过多日志。
//! 新增字段应优先保持向后兼容，旧账本允许没有 IP 事件。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。

use crate::*;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
struct AdminUserTrafficLogRow {
    id: Uuid,
    user_id: Uuid,
    access_line_id: Option<Uuid>,
    xray_user_key: String,
    traffic_source: String,
    delta_uplink: i64,
    delta_downlink: i64,
    delta_total: i64,
    billing_multiplier: f64,
    billed_uplink: i64,
    billed_downlink: i64,
    billed_bytes: i64,
    collected_at: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
    client_ip: Option<String>,
    client_ip_hash: Option<String>,
    ip_observed_at: Option<DateTime<Utc>>,
    active_connection_count: Option<i32>,
    session_status: Option<String>,
    access_line_name: Option<String>,
    protocol: Option<String>,
    transport: Option<String>,
    listen_host: Option<String>,
    listen_port: Option<i32>,
    access_node_id: Option<Uuid>,
    access_node_name: Option<String>,
    access_node_host: Option<String>,
    exit_endpoint_id: Option<Uuid>,
    exit_endpoint_name: Option<String>,
    outbound_type: Option<String>,
    exit_host: Option<String>,
    exit_port: Option<i32>,
    exit_resource_name: Option<String>,
}

impl PgStore {
    #[allow(clippy::too_many_arguments)]
    pub async fn admin_user_traffic_logs_json(
        &self,
        user_id: Uuid,
        page: i64,
        page_size: i64,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        access_line_id: Option<Uuid>,
        exit_endpoint_id: Option<Uuid>,
    ) -> Result<Value, DbError> {
        let exists =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        if !exists {
            return Err(DbError::UserNotFound);
        }

        let page = page.max(1);
        let page_size = page_size.clamp(1, 200);
        let offset = (page - 1).saturating_mul(page_size);
        let retention_policy = self.traffic_log_retention_policy().await?;
        let retention_cutoff = Utc::now() - Duration::days(retention_policy.detail_retention_days);
        let total = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM usage_ledgers ul
            WHERE ul.user_id = $1
              AND ul.traffic_source = 'access_line'
              AND COALESCE(ul.recorded_at, ul.collected_at) >= $2
              AND ($3::timestamptz IS NULL OR COALESCE(ul.recorded_at, ul.collected_at) >= $3)
              AND ($4::timestamptz IS NULL OR COALESCE(ul.recorded_at, ul.collected_at) <= $4)
              AND ($5::uuid IS NULL OR ul.access_line_id = $5)
              AND ($6::uuid IS NULL OR ul.exit_endpoint_id = $6)
            "#,
        )
        .bind(user_id)
        .bind(retention_cutoff)
        .bind(from)
        .bind(to)
        .bind(access_line_id)
        .bind(exit_endpoint_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, AdminUserTrafficLogRow>(
            r#"
            SELECT ul.id,
                   ul.user_id,
                   ul.access_line_id,
                   ul.xray_user_key,
                   ul.traffic_source,
                   ul.delta_uplink,
                   ul.delta_downlink,
                   ul.delta_total,
                   ul.billing_multiplier::float8 AS billing_multiplier,
                   ul.billed_uplink,
                   ul.billed_downlink,
                   ul.billed_bytes,
                   ul.collected_at,
                   ul.recorded_at,
                   se.client_ip,
                   se.client_ip_hash,
                   se.observed_at AS ip_observed_at,
                   se.active_connection_count,
                   se.status AS session_status,
                   al.name AS access_line_name,
                   al.protocol,
                   al.transport,
                   al.listen_host,
                   al.listen_port,
                   an.id AS access_node_id,
                   an.name AS access_node_name,
                   an.public_host AS access_node_host,
                   ee.id AS exit_endpoint_id,
                   ee.name AS exit_endpoint_name,
                   ee.outbound_type::text AS outbound_type,
                   ee.host AS exit_host,
                   ee.port AS exit_port,
                   er.name AS exit_resource_name
            FROM usage_ledgers ul
            LEFT JOIN access_lines al ON al.id = ul.access_line_id
            LEFT JOIN access_nodes an ON an.id = al.access_node_id
            LEFT JOIN exit_endpoints ee ON ee.id = ul.exit_endpoint_id
            LEFT JOIN exit_resources er ON er.id = ee.exit_resource_id
            LEFT JOIN LATERAL (
                SELECT client_ip, client_ip_hash, observed_at,
                       active_connection_count, status
                FROM access_user_session_events se
                WHERE ul.access_line_id IS NOT NULL
                  AND se.user_id = ul.user_id
                  AND se.xray_user_key = ul.xray_user_key
                  AND se.access_line_id = ul.access_line_id
                  AND se.observed_at >= COALESCE(ul.recorded_at, ul.collected_at) - interval '2 minutes'
                  AND se.observed_at <= COALESCE(ul.recorded_at, ul.collected_at) + interval '2 minutes'
                ORDER BY ABS(EXTRACT(EPOCH FROM (se.observed_at - COALESCE(ul.recorded_at, ul.collected_at)))),
                         se.observed_at DESC
                LIMIT 1
            ) se ON TRUE
            WHERE ul.user_id = $1
              AND ul.traffic_source = 'access_line'
              AND COALESCE(ul.recorded_at, ul.collected_at) >= $2
              AND ($3::timestamptz IS NULL OR COALESCE(ul.recorded_at, ul.collected_at) >= $3)
              AND ($4::timestamptz IS NULL OR COALESCE(ul.recorded_at, ul.collected_at) <= $4)
              AND ($5::uuid IS NULL OR ul.access_line_id = $5)
              AND ($6::uuid IS NULL OR ul.exit_endpoint_id = $6)
            ORDER BY COALESCE(ul.recorded_at, ul.collected_at) DESC, ul.id DESC
            LIMIT $7 OFFSET $8
            "#,
        )
        .bind(user_id)
        .bind(retention_cutoff)
        .bind(from)
        .bind(to)
        .bind(access_line_id)
        .bind(exit_endpoint_id)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let items = rows
            .into_iter()
            .map(admin_user_traffic_log_json)
            .collect::<Vec<_>>();
        Ok(json!({
            "items": items,
            "total": total.max(0),
            "page": page,
            "page_size": page_size
        }))
    }
}

fn admin_user_traffic_log_json(row: AdminUserTrafficLogRow) -> Value {
    json!({
        "id": row.id,
        "user_id": row.user_id,
        "access_line_id": row.access_line_id,
        "xray_user_key": row.xray_user_key,
        "traffic_source": row.traffic_source,
        "delta_uplink": row.delta_uplink.max(0),
        "delta_downlink": row.delta_downlink.max(0),
        "delta_total": row.delta_total.max(0),
        "billing_multiplier": row.billing_multiplier,
        "billed_uplink": row.billed_uplink.max(0),
        "billed_downlink": row.billed_downlink.max(0),
        "billed_bytes": row.billed_bytes.max(0),
        "real_gb": bytes_to_gb(row.delta_total.max(0) as u64),
        "billed_gb": bytes_to_gb(row.billed_bytes.max(0) as u64),
        "collected_at": row.collected_at,
        "recorded_at": row.recorded_at,
        "client_ip": row.client_ip.unwrap_or_default(),
        "client_ip_hash": row.client_ip_hash.unwrap_or_default(),
        "ip_observed_at": row.ip_observed_at,
        "active_connection_count": row.active_connection_count.unwrap_or_default().max(0),
        "session_status": row.session_status.unwrap_or_else(|| "unknown".to_string()),
        "access_line": {
            "id": row.access_line_id,
            "name": row.access_line_name.unwrap_or_default(),
            "protocol": row.protocol.unwrap_or_default(),
            "transport": row.transport.unwrap_or_default(),
            "listen_host": row.listen_host.unwrap_or_default(),
            "listen_port": row.listen_port.unwrap_or_default()
        },
        "access_node": {
            "id": row.access_node_id,
            "name": row.access_node_name.unwrap_or_default(),
            "public_host": row.access_node_host.unwrap_or_default()
        },
        "exit_endpoint": {
            "id": row.exit_endpoint_id,
            "name": row.exit_endpoint_name.unwrap_or_default(),
            "outbound_type": row.outbound_type.unwrap_or_default(),
            "host": row.exit_host.unwrap_or_default(),
            "port": row.exit_port.unwrap_or_default(),
            "resource_name": row.exit_resource_name.unwrap_or_default()
        }
    })
}
