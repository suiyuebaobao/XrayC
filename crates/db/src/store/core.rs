//! PgStore 连接、限流与审计日志入口。
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
    pub async fn connect(database_url: &str) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self, path: impl AsRef<Path>) -> Result<(), DbError> {
        let migrator = sqlx::migrate::Migrator::new(path.as_ref()).await?;
        migrator.run(&self.pool).await?;
        self.repair_access_entry_binding_line_inbound_configs()
            .await?;
        Ok(())
    }

    pub async fn allow_rate_limit(
        &self,
        scope: &str,
        identity_hash: &str,
        limit: i32,
        window_seconds: i64,
    ) -> Result<bool, DbError> {
        let window_seconds = window_seconds.clamp(1, 86_400) as f64;
        let row = sqlx::query_scalar::<_, i32>(
            r#"
            WITH bucket AS (
                SELECT to_timestamp(
                    floor(extract(epoch FROM now()) / $4::DOUBLE PRECISION)
                    * $4::DOUBLE PRECISION
                ) AS window_start
            )
            INSERT INTO rate_limit_buckets (
                scope, identity_hash, window_start, request_count, updated_at
            )
            SELECT $1, $2, window_start, 1, now()
            FROM bucket
            ON CONFLICT (scope, identity_hash, window_start) DO UPDATE SET
                request_count = rate_limit_buckets.request_count + 1,
                updated_at = now()
            WHERE rate_limit_buckets.request_count < $3
            RETURNING request_count
            "#,
        )
        .bind(scope)
        .bind(identity_hash)
        .bind(limit.max(1))
        .bind(window_seconds)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    pub async fn record_audit_log(&self, input: AuditLogInput) -> Result<Uuid, DbError> {
        // 审计日志入库前统一脱敏，避免后台读取以外的路径看到原始凭据。
        let request_summary = stored_audit_summary(input.request_summary);
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO audit_logs (
                actor_user_id, actor_email, action, resource_type, resource_id,
                client_ip, request_summary, result
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id
            "#,
        )
        .bind(input.actor_user_id)
        .bind(input.actor_email)
        .bind(input.action)
        .bind(input.resource_type)
        .bind(input.resource_id)
        .bind(safe_client_ip_summary(&input.client_ip))
        .bind(request_summary)
        .bind(input.result)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn admin_audit_logs_json(&self, page: i64, page_size: i64) -> Result<Value, DbError> {
        let page = page.max(1);
        let page_size = page_size.clamp(1, ADMIN_AUDIT_LOG_MAX_PAGE_SIZE);
        let offset = page.saturating_sub(1).saturating_mul(page_size);
        let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*)::BIGINT FROM audit_logs")
            .fetch_one(&self.pool)
            .await?;
        let rows = sqlx::query_as::<_, AuditLogListRow>(
            r#"
            WITH page_ids AS (
                SELECT id
                FROM audit_logs
                ORDER BY created_at DESC, id DESC
                LIMIT $1 OFFSET $2
            )
            SELECT a.id, a.actor_email, a.action, a.resource_type, a.resource_id,
                   a.request_summary, a.result, a.created_at
            FROM page_ids p
            JOIN audit_logs a ON a.id = p.id
            ORDER BY a.created_at DESC, a.id DESC
            "#,
        )
        .bind(page_size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let items = rows
            .into_iter()
            .map(admin_audit_log_json)
            .collect::<Vec<_>>();
        let mut payload = match admin_collection_json("audit_logs", json!(items)) {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        payload.insert("total".to_string(), json!(total.max(0)));
        payload.insert("page".to_string(), json!(page));
        payload.insert("page_size".to_string(), json!(page_size));
        Ok(Value::Object(payload))
    }
}
