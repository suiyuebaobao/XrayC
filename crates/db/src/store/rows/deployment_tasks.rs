//! 部署任务查询行模型。
//! 只保存脱敏后的部署状态和进度，不保存服务器密码、明文 token 或命令。
//! 任务 token 在数据库中仅保存 hash，明文只在生成安装说明时返回一次。
//! 前端列表和脚本回传接口都通过这些行模型组装 JSON。
//! 新增字段应先确认是否会泄露目标服务器敏感信息。
//! 所有时间字段直接使用 PostgreSQL 的 timestamptz。
//! JSON 字段只允许保存步骤、结果摘要和脱敏元数据。
//! 本文件不包含 SQL 查询，只承载 FromRow 结构。
//! 调用方负责状态机和权限校验。
//! 文件头部中文注释满足仓库规则。

use crate::*;
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub(crate) struct DeploymentTaskRow {
    pub(crate) id: Uuid,
    pub(crate) kind: String,
    pub(crate) status: String,
    pub(crate) target_type: String,
    pub(crate) target_id: Option<Uuid>,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) current_step: String,
    pub(crate) progress_percent: i32,
    pub(crate) safe_metadata: Value,
    pub(crate) steps: Value,
    pub(crate) result: Value,
    pub(crate) error_summary: String,
    pub(crate) created_by_user_id: Option<Uuid>,
    pub(crate) created_by_email: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) completed_at: Option<DateTime<Utc>>,
}
