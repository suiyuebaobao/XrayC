//! 后台用户设备/IP 观察列表。
//! 本模块只汇总订阅拉取 IP 与真实节点使用 IP。
//! 它不改变唯一订阅 URL 或用户凭据模型。
//! 有有效哈希时按哈希去重，无哈希时回退明文 IP。
//! 订阅拉取事件来自 subscription_pull_events。
//! 节点使用事件来自 access_user_session_events。
//! 返回字段服务后台排障，不暴露到普通用户接口。
//! 本模块不写入用户授权、不下发 agent 配置。
//! 所有 SQL 按用户限定，避免跨用户混入。
//! 本头部满足前十行中文注释约束。

use crate::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
struct AdminUserDeviceRow {
    client_ip: String,
    client_ip_hash: String,
    source: String,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    subscription_pull_count: i64,
    node_use_count: i64,
    last_access_line_id: Option<Uuid>,
    last_access_line_name: Option<String>,
    last_access_line_protocol: Option<String>,
    last_access_line_transport: Option<String>,
    last_access_node_id: Option<Uuid>,
    last_access_node_name: Option<String>,
    last_access_node_host: Option<String>,
    last_session_status: Option<String>,
    last_active_connection_count: Option<i32>,
}

impl PgStore {
    pub async fn record_subscription_pull_event_for_user(
        &self,
        user_id: Uuid,
        client_ip: &str,
        client_ip_hash: &str,
        user_agent: &str,
    ) -> Result<(), DbError> {
        let client_ip = normalize_client_ip(client_ip)?;
        let client_ip_hash = normalize_client_ip_hash(&client_ip, client_ip_hash)?;
        sqlx::query(
            r#"
            INSERT INTO subscription_pull_events (
                user_id, token_hash, client_ip, client_ip_hash, user_agent, observed_at
            )
            VALUES ($1, '', $2, $3, $4, now())
            "#,
        )
        .bind(user_id)
        .bind(client_ip)
        .bind(client_ip_hash)
        .bind(user_agent.chars().take(256).collect::<String>())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn record_subscription_pull_event_for_token(
        &self,
        token: &str,
        client_ip: &str,
        client_ip_hash: &str,
        user_agent: &str,
    ) -> Result<(), DbError> {
        let token_hash = agent_token_hash(token);
        let Some(user_id) = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT user_id
            FROM subscription_tokens
            WHERE token_hash = $1
              AND revoked_at IS NULL
              AND expires_at > now()
            LIMIT 1
            "#,
        )
        .bind(&token_hash)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(());
        };
        let client_ip = normalize_client_ip(client_ip)?;
        let client_ip_hash = normalize_client_ip_hash(&client_ip, client_ip_hash)?;
        sqlx::query(
            r#"
            INSERT INTO subscription_pull_events (
                user_id, token_hash, client_ip, client_ip_hash, user_agent, observed_at
            )
            VALUES ($1, $2, $3, $4, $5, now())
            "#,
        )
        .bind(user_id)
        .bind(token_hash)
        .bind(client_ip)
        .bind(client_ip_hash)
        .bind(user_agent.chars().take(256).collect::<String>())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn admin_user_devices_json(
        &self,
        user_id: Uuid,
        page: i64,
        page_size: i64,
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
        let total = sqlx::query_scalar::<_, i64>(
            r#"
            WITH events AS (
                SELECT CASE WHEN client_ip_hash <> '' THEN 'hash:' || client_ip_hash ELSE 'ip:' || client_ip END AS dedup_key
                FROM subscription_pull_events
                WHERE user_id = $1 AND (client_ip <> '' OR client_ip_hash <> '')
                UNION ALL
                SELECT CASE WHEN client_ip_hash <> '' THEN 'hash:' || client_ip_hash ELSE 'ip:' || client_ip END AS dedup_key
                FROM access_user_session_events
                WHERE user_id = $1 AND (client_ip <> '' OR client_ip_hash <> '')
            )
            SELECT COUNT(DISTINCT dedup_key)::bigint FROM events
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, AdminUserDeviceRow>(
            r#"
            WITH events AS (
                SELECT CASE WHEN spe.client_ip_hash <> '' THEN 'hash:' || spe.client_ip_hash ELSE 'ip:' || spe.client_ip END AS dedup_key,
                       spe.client_ip, spe.client_ip_hash, 'subscription_pull'::text AS source,
                       spe.observed_at, NULL::uuid AS access_line_id,
                       NULL::integer AS active_connection_count, NULL::text AS session_status
                FROM subscription_pull_events spe
                WHERE spe.user_id = $1 AND (spe.client_ip <> '' OR spe.client_ip_hash <> '')
                UNION ALL
                SELECT CASE WHEN se.client_ip_hash <> '' THEN 'hash:' || se.client_ip_hash ELSE 'ip:' || se.client_ip END AS dedup_key,
                       se.client_ip, se.client_ip_hash, 'node_use'::text AS source,
                       se.observed_at, se.access_line_id,
                       se.active_connection_count, se.status AS session_status
                FROM access_user_session_events se
                WHERE se.user_id = $1 AND (se.client_ip <> '' OR se.client_ip_hash <> '')
            ),
            grouped AS (
                SELECT dedup_key,
                       COALESCE(MAX(NULLIF(client_ip, '')), '') AS client_ip,
                       COALESCE(MAX(NULLIF(client_ip_hash, '')), '') AS client_ip_hash,
                       MIN(observed_at) AS first_seen_at,
                       MAX(observed_at) AS last_seen_at,
                       COUNT(*) FILTER (WHERE source = 'subscription_pull')::bigint AS subscription_pull_count,
                       COUNT(*) FILTER (WHERE source = 'node_use')::bigint AS node_use_count
                FROM events
                GROUP BY dedup_key
            ),
            latest AS (
                SELECT DISTINCT ON (dedup_key)
                       dedup_key, access_line_id,
                       active_connection_count, session_status
                FROM events
                WHERE source = 'node_use'
                ORDER BY dedup_key, observed_at DESC
            )
            SELECT g.client_ip,
                   g.client_ip_hash,
                   CASE
                       WHEN g.subscription_pull_count > 0 AND g.node_use_count > 0 THEN 'both'
                       WHEN g.subscription_pull_count > 0 THEN 'subscription_pull'
                       WHEN g.node_use_count > 0 THEN 'node_use'
                       ELSE 'unknown'
                   END AS source,
                   g.first_seen_at,
                   g.last_seen_at,
                   g.subscription_pull_count,
                   g.node_use_count,
                   al.id AS last_access_line_id,
                   al.name AS last_access_line_name,
                   al.protocol AS last_access_line_protocol,
                   al.transport AS last_access_line_transport,
                   an.id AS last_access_node_id,
                   an.name AS last_access_node_name,
                   an.public_host AS last_access_node_host,
                   latest.session_status AS last_session_status,
                   latest.active_connection_count AS last_active_connection_count
            FROM grouped g
            LEFT JOIN latest ON latest.dedup_key = g.dedup_key
            LEFT JOIN access_lines al ON al.id = latest.access_line_id
            LEFT JOIN access_nodes an ON an.id = al.access_node_id
            ORDER BY g.last_seen_at DESC, g.dedup_key ASC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(admin_user_device_json)
            .collect::<Vec<_>>();
        Ok(json!({
            "items": items,
            "total": total.max(0),
            "page": page,
            "page_size": page_size
        }))
    }
}

fn normalize_client_ip(value: &str) -> Result<String, DbError> {
    let value = value.trim();
    if value.is_empty() || value.parse::<std::net::IpAddr>().is_ok() {
        return Ok(value.chars().take(128).collect());
    }
    Err(DbError::InvalidInput("client_ip 格式无效".to_string()))
}

fn normalize_client_ip_hash(client_ip: &str, value: &str) -> Result<String, DbError> {
    let value = value.trim();
    if is_valid_client_ip_hash(value) {
        return Ok(value.to_string());
    }
    if !client_ip.is_empty() {
        return Ok(format!("sha256:{:x}", Sha256::digest(client_ip.as_bytes())));
    }
    Err(DbError::InvalidInput(
        "client_ip_hash 必须是 sha256:<64位hex>".to_string(),
    ))
}

fn admin_user_device_json(row: AdminUserDeviceRow) -> Value {
    json!({
        "client_ip": row.client_ip,
        "client_ip_hash": row.client_ip_hash,
        "source": row.source,
        "first_seen_at": row.first_seen_at,
        "last_seen_at": row.last_seen_at,
        "subscription_pull_count": row.subscription_pull_count.max(0),
        "node_use_count": row.node_use_count.max(0),
        "last_access_line": {
            "id": row.last_access_line_id,
            "name": row.last_access_line_name.unwrap_or_default(),
            "protocol": row.last_access_line_protocol.unwrap_or_default(),
            "transport": row.last_access_line_transport.unwrap_or_default()
        },
        "last_access_node": {
            "id": row.last_access_node_id,
            "name": row.last_access_node_name.unwrap_or_default(),
            "public_host": row.last_access_node_host.unwrap_or_default()
        },
        "last_session_status": row.last_session_status.unwrap_or_else(|| "unknown".to_string()),
        "last_active_connection_count": row.last_active_connection_count.unwrap_or_default().max(0)
    })
}
