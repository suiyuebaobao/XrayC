//! 监控中心·节点运行态指标数据层(时序写入 + 读模型挂载 + 库存储占用)。
//! 本文件承载 PgStore 在监控中心数据层的一个业务切面:节点 CPU/内存/磁盘时序与库占用。
//! record 单条 INSERT 进 node_runtime_metrics(append-only,非 upsert),写入侧钳制非法值。
//! attach 用 DISTINCT ON 取每节点最新指标,merge 进读模型 access_nodes[].runtime_metrics(无数据 null)。
//! database_storage_json 返回库总字节 + 前 10 大表字节,供监控中心存储占用展示。
//! 不在这里引入 API、前端或脚本层的行为改动;无 PG 的内存模式不持有 PgStore,故天然 no-op。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文,方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use crate::*;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

// CPU 使用率千分比上限(100% = 100000‰),写入侧据此钳制非法上报值。
const CPU_PCT_MILLI_MAX: i32 = 100_000;

/// 监控中心·节点运行态指标上报入参。
/// access-agent 经心跳上报本机 CPU/内存/磁盘占用,record 单条 INSERT 进时序表。
/// cpu_pct_milli 为 CPU 使用率千分比(写入侧钳制到 0..100000);字节列写入侧钳制 >=0。
/// collected_at_unix 为采样 Unix 时间戳(秒),写入侧转 TIMESTAMPTZ 落 collected_at。
#[derive(Debug, Clone, Default)]
pub struct NodeRuntimeMetricsReport {
    pub cpu_pct_milli: i32,
    pub mem_used_bytes: i64,
    pub mem_total_bytes: i64,
    pub disk_used_bytes: i64,
    pub disk_total_bytes: i64,
    pub collected_at_unix: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct NodeRuntimeMetricLatestRow {
    access_node_id: Uuid,
    cpu_pct_milli: i32,
    mem_used_bytes: i64,
    mem_total_bytes: i64,
    disk_used_bytes: i64,
    disk_total_bytes: i64,
    collected_at: DateTime<Utc>,
}

impl PgStore {
    /// 记录一条节点运行态指标(append-only 时序,单条 INSERT)。
    /// 写入前钳制非法值:cpu_pct_milli 限 [0,100000]、字节列限 >=0,防止 agent 上报越界污染读模型。
    /// collected_at_unix 转 TIMESTAMPTZ;非法时间戳回退 now(),不让一条坏采样阻塞整批上报。
    pub async fn record_node_runtime_metrics(
        &self,
        node_id: Uuid,
        report: NodeRuntimeMetricsReport,
    ) -> Result<(), DbError> {
        let cpu_pct_milli = report.cpu_pct_milli.clamp(0, CPU_PCT_MILLI_MAX);
        let mem_used_bytes = report.mem_used_bytes.max(0);
        let mem_total_bytes = report.mem_total_bytes.max(0);
        let disk_used_bytes = report.disk_used_bytes.max(0);
        let disk_total_bytes = report.disk_total_bytes.max(0);
        let collected_at =
            DateTime::from_timestamp(report.collected_at_unix, 0).unwrap_or_else(Utc::now);

        sqlx::query(
            r#"
            INSERT INTO node_runtime_metrics (
                access_node_id, cpu_pct_milli,
                mem_used_bytes, mem_total_bytes,
                disk_used_bytes, disk_total_bytes,
                collected_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(node_id)
        .bind(cpu_pct_milli)
        .bind(mem_used_bytes)
        .bind(mem_total_bytes)
        .bind(disk_used_bytes)
        .bind(disk_total_bytes)
        .bind(collected_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 把每节点最新一条运行态指标挂进读模型 access_nodes[].runtime_metrics。
    /// 用 DISTINCT ON (access_node_id) ORDER BY collected_at DESC 取每节点最新;无数据节点给 null。
    /// 仿 attach_latest_line_runtime,按 access_nodes[i].id(UUID)命中,缺键不漏挂、不串其他节点。
    pub async fn attach_latest_node_runtime(&self, value: &mut Value) -> Result<(), DbError> {
        let metrics = sqlx::query_as::<_, NodeRuntimeMetricLatestRow>(
            r#"
            SELECT DISTINCT ON (access_node_id)
                access_node_id, cpu_pct_milli,
                mem_used_bytes, mem_total_bytes,
                disk_used_bytes, disk_total_bytes,
                collected_at
            FROM node_runtime_metrics
            ORDER BY access_node_id, collected_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| (row.access_node_id, row))
        .collect::<HashMap<_, _>>();

        if let Some(nodes) = value.get_mut("access_nodes").and_then(Value::as_array_mut) {
            for node in nodes {
                let node_id = node
                    .get("id")
                    .or_else(|| node.get("uuid"))
                    .and_then(Value::as_str)
                    .and_then(|raw| Uuid::parse_str(raw).ok());
                let Some(object) = node.as_object_mut() else {
                    continue;
                };
                let runtime_metrics = node_id
                    .and_then(|node_id| metrics.get(&node_id))
                    .map(|metric| {
                        json!({
                            "cpu_pct_milli": metric.cpu_pct_milli,
                            "mem_used_bytes": metric.mem_used_bytes,
                            "mem_total_bytes": metric.mem_total_bytes,
                            "disk_used_bytes": metric.disk_used_bytes,
                            "disk_total_bytes": metric.disk_total_bytes,
                            "collected_at": metric.collected_at,
                        })
                    })
                    .unwrap_or(Value::Null);
                object.insert("runtime_metrics".to_string(), runtime_metrics);
            }
        }
        Ok(())
    }

    /// 返回数据库存储占用:库总字节 + 占用前 10 大表的字节明细。
    /// total_bytes 取 pg_database_size(current_database());tables 用 pg_total_relation_size 排序取前 10。
    /// 供监控中心展示磁盘占用与表级分布,只读不改任何业务数据。
    pub async fn database_storage_json(&self) -> Result<Value, DbError> {
        let total_bytes =
            sqlx::query_scalar::<_, i64>(r#"SELECT pg_database_size(current_database())::BIGINT"#)
                .fetch_one(&self.pool)
                .await?;

        let tables = sqlx::query_as::<_, (String, i64)>(
            r#"
            SELECT c.relname AS name,
                   pg_total_relation_size(c.oid)::BIGINT AS bytes
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relkind = 'r'
              AND n.nspname = 'public'
            ORDER BY pg_total_relation_size(c.oid) DESC
            LIMIT 10
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(name, bytes)| json!({ "name": name, "bytes": bytes }))
        .collect::<Vec<_>>();

        Ok(json!({
            "total_bytes": total_bytes,
            "tables": tables,
        }))
    }
}
