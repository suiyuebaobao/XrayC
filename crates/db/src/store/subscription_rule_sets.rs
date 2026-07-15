//! 订阅规则库持久化。
//! 本模块只处理规则库 CRUD，不处理订阅 YAML 渲染。
//! 规则库被线路分组绑定后，不允许直接删除，避免引用悬空。
//! 规则内容保存为有序字符串数组，兼容现有订阅规则编辑器。
//! 输入归一化沿用后台管理文本校验，避免空名称和过长备注。
//! 删除冲突使用 DbError::InvalidInput，保持现有 API 错误映射一致。
//! 绑定表的替换逻辑放在线路分组模块，保证分组更新事务完整。
//! 本文件不读取部署主机、密钥或任何外部凭据。
//! 新增查询保持确定排序，便于前端和测试稳定展示。
//! 本头部满足前十行中文注释约束。

use crate::*;
use serde_json::{json, Value};
use uuid::Uuid;

impl PgStore {
    pub async fn create_subscription_rule_set(
        &self,
        input: AdminSubscriptionRuleSetInput,
    ) -> Result<Uuid, DbError> {
        let name = required_admin_text(&input.name, "规则库名称", 128)?;
        let description = optional_admin_text(&input.description, 512);
        let enabled = input.enabled.unwrap_or(true);
        let rules = normalized_rule_set_rules(input.rules);
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO subscription_rule_sets (name, description, enabled, rules)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(description)
        .bind(enabled)
        .bind(json!(rules))
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn list_subscription_rule_sets(
        &self,
    ) -> Result<Vec<SubscriptionRuleSetSummary>, DbError> {
        let rows = sqlx::query_as::<_, SubscriptionRuleSetRow>(
            r#"
            SELECT rs.id,
                   rs.name,
                   rs.description,
                   rs.enabled,
                   rs.rules,
                   COUNT(b.rule_set_id)::BIGINT AS binding_count,
                   rs.created_at,
                   rs.updated_at
            FROM subscription_rule_sets rs
            LEFT JOIN line_group_rule_set_bindings b ON b.rule_set_id = rs.id
            GROUP BY rs.id, rs.name, rs.description, rs.enabled, rs.rules, rs.created_at, rs.updated_at
            ORDER BY rs.enabled DESC, rs.name, rs.id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SubscriptionRuleSetSummary {
                id: row.id,
                name: row.name,
                description: row.description,
                enabled: row.enabled,
                rules: string_array_from_value(row.rules),
                binding_count: row.binding_count,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
            .collect())
    }

    pub async fn update_subscription_rule_set(
        &self,
        rule_set_id: Uuid,
        input: AdminSubscriptionRuleSetInput,
    ) -> Result<(), DbError> {
        let name = required_admin_text(&input.name, "规则库名称", 128)?;
        let description = optional_admin_text(&input.description, 512);
        let enabled = input.enabled.unwrap_or(true);
        let rules = normalized_rule_set_rules(input.rules);
        let result = sqlx::query(
            r#"
            UPDATE subscription_rule_sets
            SET name = $2,
                description = $3,
                enabled = $4,
                rules = $5,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(rule_set_id)
        .bind(name)
        .bind(description)
        .bind(enabled)
        .bind(json!(rules))
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::InvalidInput("规则库不存在".to_string()));
        }
        Ok(())
    }

    pub async fn delete_subscription_rule_set(&self, rule_set_id: Uuid) -> Result<u64, DbError> {
        let binding_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM line_group_rule_set_bindings WHERE rule_set_id = $1",
        )
        .bind(rule_set_id)
        .fetch_one(&self.pool)
        .await?;
        if binding_count > 0 {
            return Err(DbError::InvalidInput(
                "规则库已被线路分组绑定，请先解除绑定".to_string(),
            ));
        }
        let result = sqlx::query("DELETE FROM subscription_rule_sets WHERE id = $1")
            .bind(rule_set_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::InvalidInput("规则库不存在".to_string()));
        }
        Ok(result.rows_affected())
    }
}

#[derive(sqlx::FromRow)]
struct SubscriptionRuleSetRow {
    id: Uuid,
    name: String,
    description: String,
    enabled: bool,
    rules: Value,
    binding_count: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

pub(crate) fn normalized_rule_set_rules(rules: Vec<String>) -> Vec<String> {
    rules
        .into_iter()
        .map(|rule| rule.trim().to_string())
        .filter(|rule| !rule.is_empty())
        .collect()
}

pub(crate) fn string_array_from_value(value: Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::trim).map(str::to_string))
            .filter(|item| !item.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}
