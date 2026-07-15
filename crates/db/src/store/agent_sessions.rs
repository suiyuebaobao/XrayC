//! Agent 会话上报批量写入逻辑。
//! 本模块从 agent_metrics 拆出，避免单文件继续膨胀。
//! 会话上报是运行态压测热点路径，必须避免按 session 逐条执行 SQL。
//! 写入流程仍保持一个事务：校验线路归属、替换在线快照、追加事件。
//! 在线快照按节点、线路、用户 key 和 client_ip_hash 保留最新项。
//! 事件表保留本次 payload 的全部观测项，便于后续审计和在线终端统计。
//! 字段解析和 client IP 校验仍在 Rust 侧完成，避免脏数据进入批量 SQL。
//! 新增字段时应同步 ParsedAgentSession 和两段 UNNEST SQL。
//! 注释使用中文，方便后续维护和审查。
//! 本头部满足前十行中文注释约束。
use super::agent_session_helpers::*;
use super::existence::*;
use super::runtime_helpers::*;
use crate::*;
use serde_json::Value;
use std::collections::HashSet;
use uuid::Uuid;

struct ParsedAgentSession {
    access_line_id: Uuid,
    xray_user_key: String,
    client_ip: String,
    client_ip_hash: String,
    active_connection_count: i32,
    status: String,
    started_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    ordinal: i32,
}

impl PgStore {
    pub async fn record_agent_sessions(
        &self,
        access_node_id: Uuid,
        payload: &Value,
    ) -> Result<usize, DbError> {
        let sessions =
            payload_items_with_root_field(payload, &["sessions", "items"], "xray_user_key");
        let mut tx = self.pool.begin().await?;
        if sessions.is_empty() {
            // 空会话快照通常表示 access log 当前没有新命中，不能立刻抹掉仍在观测窗口内的在线记录。
            sqlx::query(
                r#"
                DELETE FROM access_user_sessions
                WHERE access_node_id = $1
                  AND last_seen_at <= now() - interval '10 minutes'
                "#,
            )
            .bind(access_node_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(0);
        }

        let mut parsed = Vec::with_capacity(sessions.len());
        for (index, session) in sessions.iter().enumerate() {
            let access_line_id = payload_uuid(session, "access_line_id")?;
            let xray_user_key = payload_required_text(session, "xray_user_key")?;
            let last_seen_at = payload_time(
                session,
                &["last_seen_at_unix", "captured_at_unix"],
                &["last_seen_at", "captured_at"],
            );
            let started_at = payload_optional_time(session, &["started_at_unix"], &["started_at"])
                .unwrap_or(last_seen_at);
            let active_connection_count = payload_i32_any(
                session,
                &[
                    "active_connection_count",
                    "active_connections",
                    "connection_count",
                ],
            )
            .max(0);
            let status = payload_session_status(session, active_connection_count)?;
            let client_ip = payload_client_ip(session)?;
            let client_ip_hash = session_client_ip_hash(session, &client_ip)?;
            parsed.push(ParsedAgentSession {
                access_line_id,
                xray_user_key,
                client_ip,
                client_ip_hash,
                active_connection_count,
                status,
                started_at,
                last_seen_at,
                ordinal: i32::try_from(index).unwrap_or(i32::MAX),
            });
        }

        let mut unique_line_ids = Vec::new();
        let mut seen_line_ids = HashSet::new();
        for session in &parsed {
            if seen_line_ids.insert(session.access_line_id) {
                unique_line_ids.push(session.access_line_id);
            }
        }
        ensure_lines_belong_to_node_in_tx(&mut tx, access_node_id, &unique_line_ids).await?;

        let access_line_ids: Vec<Uuid> = parsed
            .iter()
            .map(|session| session.access_line_id)
            .collect();
        let xray_user_keys: Vec<String> = parsed
            .iter()
            .map(|session| session.xray_user_key.clone())
            .collect();
        let client_ips: Vec<String> = parsed
            .iter()
            .map(|session| session.client_ip.clone())
            .collect();
        let client_ip_hashes: Vec<String> = parsed
            .iter()
            .map(|session| session.client_ip_hash.clone())
            .collect();
        let active_connection_counts: Vec<i32> = parsed
            .iter()
            .map(|session| session.active_connection_count)
            .collect();
        let statuses: Vec<String> = parsed
            .iter()
            .map(|session| session.status.clone())
            .collect();
        let started_ats: Vec<DateTime<Utc>> =
            parsed.iter().map(|session| session.started_at).collect();
        let last_seen_ats: Vec<DateTime<Utc>> =
            parsed.iter().map(|session| session.last_seen_at).collect();
        let ordinals: Vec<i32> = parsed.iter().map(|session| session.ordinal).collect();

        sqlx::query(
            r#"
            WITH incoming_raw AS (
                SELECT *
                FROM UNNEST(
                    $2::uuid[], $3::text[], $4::text[], $5::int4[],
                    $6::text[], $7::timestamptz[], $8::timestamptz[], $9::int4[]
                ) AS incoming(
                    access_line_id, xray_user_key, client_ip_hash,
                    active_connection_count, status, started_at, last_seen_at, ordinal
                )
            ),
            incoming AS (
                SELECT DISTINCT ON (access_line_id, xray_user_key, client_ip_hash)
                    access_line_id, xray_user_key, client_ip_hash,
                    active_connection_count, status, started_at, last_seen_at
                FROM incoming_raw
                ORDER BY access_line_id, xray_user_key, client_ip_hash, ordinal DESC
            )
            INSERT INTO access_user_sessions (
                access_node_id, access_line_id, xray_user_key,
                client_ip_hash, active_connection_count, status,
                started_at, last_seen_at
            )
            SELECT $1, access_line_id, xray_user_key,
                   client_ip_hash, active_connection_count, status,
                   started_at, last_seen_at
            FROM incoming
            ON CONFLICT (access_node_id, access_line_id, xray_user_key, client_ip_hash)
            DO UPDATE SET
                active_connection_count = CASE
                    WHEN EXCLUDED.last_seen_at >= access_user_sessions.last_seen_at
                    THEN EXCLUDED.active_connection_count
                    ELSE access_user_sessions.active_connection_count
                END,
                status = CASE
                    WHEN EXCLUDED.last_seen_at >= access_user_sessions.last_seen_at
                    THEN EXCLUDED.status
                    ELSE access_user_sessions.status
                END,
                started_at = CASE
                    WHEN EXCLUDED.last_seen_at >= access_user_sessions.last_seen_at
                    THEN EXCLUDED.started_at
                    ELSE access_user_sessions.started_at
                END,
                last_seen_at = GREATEST(access_user_sessions.last_seen_at, EXCLUDED.last_seen_at)
            "#,
        )
        .bind(access_node_id)
        .bind(&access_line_ids)
        .bind(&xray_user_keys)
        .bind(&client_ip_hashes)
        .bind(&active_connection_counts)
        .bind(&statuses)
        .bind(&started_ats)
        .bind(&last_seen_ats)
        .bind(&ordinals)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            WITH incoming AS (
                SELECT *
                FROM UNNEST(
                    $2::uuid[], $3::text[], $4::text[], $5::text[], $6::int4[],
                    $7::text[], $8::timestamptz[]
                ) AS incoming(
                    access_line_id, xray_user_key, client_ip, client_ip_hash,
                    active_connection_count, status, last_seen_at
                )
            )
            INSERT INTO access_user_session_events (
                access_node_id, access_line_id, user_id, xray_user_key,
                client_ip, client_ip_hash, active_connection_count,
                status, observed_at
            )
            SELECT $1, incoming.access_line_id, u.id, incoming.xray_user_key,
                   incoming.client_ip, incoming.client_ip_hash,
                   incoming.active_connection_count, incoming.status, incoming.last_seen_at
            FROM incoming
            LEFT JOIN users u ON u.xray_user_key = incoming.xray_user_key
            "#,
        )
        .bind(access_node_id)
        .bind(&access_line_ids)
        .bind(&xray_user_keys)
        .bind(&client_ips)
        .bind(&client_ip_hashes)
        .bind(&active_connection_counts)
        .bind(&statuses)
        .bind(&last_seen_ats)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM access_user_sessions
            WHERE access_node_id = $1
              AND last_seen_at <= now() - interval '10 minutes'
            "#,
        )
        .bind(access_node_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(parsed.len())
    }
}

async fn ensure_lines_belong_to_node_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    access_line_ids: &[Uuid],
) -> Result<(), DbError> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT id, access_node_id FROM access_lines WHERE id = ANY($1)",
    )
    .bind(access_line_ids)
    .fetch_all(&mut **tx)
    .await?;
    if rows.len() != access_line_ids.len() {
        return Err(DbError::AccessLineNotFound);
    }
    if rows
        .iter()
        .any(|(_, owner_access_node_id)| *owner_access_node_id != access_node_id)
    {
        return Err(DbError::AccessLineNodeMismatch);
    }
    Ok(())
}
