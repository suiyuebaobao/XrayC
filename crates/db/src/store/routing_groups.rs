//! 线路组管理接口。
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
    pub async fn create_admin_line_group(
        &self,
        input: AdminLineGroupInput,
    ) -> Result<Uuid, DbError> {
        let name = required_admin_text(&input.name, "线路组名称", 128)?;
        let country_code = optional_admin_text(&input.country_code, 8);
        let icon = optional_admin_text(&input.icon, 256);
        let group_level = normalize_line_group_level(input.group_level.as_deref())?;
        let sort_weight = input.sort_weight.unwrap_or(100).clamp(0, 1_000_000);
        let enabled = input.enabled.unwrap_or(true);
        let dedicated_rules = normalized_dedicated_rules(input.dedicated_rules.unwrap_or_default());
        let mut tx = self.pool.begin().await?;
        validate_line_group_parent_in_tx(&mut tx, None, &group_level, input.parent_group_id)
            .await?;
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO line_groups (
                name, country_code, icon, group_level,
                parent_group_id, sort_weight, billing_multiplier, enabled,
                dedicated_rules
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(country_code)
        .bind(icon)
        .bind(group_level)
        .bind(input.parent_group_id)
        .bind(sort_weight)
        .bind(1.0_f64)
        .bind(enabled)
        .bind(json!(dedicated_rules))
        .fetch_one(&mut *tx)
        .await?;
        if let Some(bindings) = input.rule_set_bindings {
            replace_line_group_rule_set_bindings_in_tx(&mut tx, id, bindings).await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    pub async fn update_admin_line_group(
        &self,
        line_group_id: Uuid,
        input: AdminLineGroupInput,
    ) -> Result<(), DbError> {
        let name = required_admin_text(&input.name, "线路组名称", 128)?;
        let country_code = optional_admin_text(&input.country_code, 8);
        let icon = optional_admin_text(&input.icon, 256);
        let mut tx = self.pool.begin().await?;
        let Some((current_level, _current_parent)) = sqlx::query_as::<_, (String, Option<Uuid>)>(
            "SELECT group_level, parent_group_id FROM line_groups WHERE id = $1 FOR UPDATE",
        )
        .bind(line_group_id)
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Err(DbError::InvalidInput("线路组不存在".to_string()));
        };
        let group_level = normalize_line_group_level(Some(
            input
                .group_level
                .as_deref()
                .unwrap_or(current_level.as_str()),
        ))?;
        let parent_group_id = None;
        let dedicated_rules = input.dedicated_rules.map(normalized_dedicated_rules);
        validate_line_group_parent_in_tx(
            &mut tx,
            Some(line_group_id),
            &group_level,
            parent_group_id,
        )
        .await?;
        let sort_weight = input.sort_weight.map(|value| value.clamp(0, 1_000_000));
        let result = sqlx::query(
            r#"
            UPDATE line_groups
            SET name = $2,
                country_code = $3,
                icon = $4,
                group_level = $5,
                parent_group_id = $6,
                sort_weight = COALESCE($7, sort_weight),
                enabled = COALESCE($8, enabled),
                dedicated_rules = COALESCE($9, dedicated_rules)
            WHERE id = $1
            "#,
        )
        .bind(line_group_id)
        .bind(name)
        .bind(country_code)
        .bind(icon)
        .bind(group_level)
        .bind(parent_group_id)
        .bind(sort_weight)
        .bind(input.enabled)
        .bind(dedicated_rules.map(|rules| json!(rules)))
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::InvalidInput("线路组不存在".to_string()));
        }
        if let Some(bindings) = input.rule_set_bindings {
            replace_line_group_rule_set_bindings_in_tx(&mut tx, line_group_id, bindings).await?;
        }
        if input.group_level.is_some() || input.parent_group_id.is_some() || input.enabled.is_some()
        {
            sqlx::query("DELETE FROM user_access_line_assignments")
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM user_exit_assignments")
                .execute(&mut *tx)
                .await?;
            mark_all_nodes_dirty_in_tx(&mut tx, "admin_updated_line_group_hierarchy").await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_admin_line_group(&self, line_group_id: Uuid) -> Result<u64, DbError> {
        let mut tx = self.pool.begin().await?;
        ensure_line_group_can_be_deleted_in_tx(&mut tx, line_group_id).await?;
        let result = sqlx::query("DELETE FROM line_groups WHERE id = $1")
            .bind(line_group_id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::InvalidInput("线路组不存在".to_string()));
        }
        sqlx::query("DELETE FROM user_access_line_assignments")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM user_exit_assignments")
            .execute(&mut *tx)
            .await?;
        mark_all_nodes_dirty_in_tx(&mut tx, "admin_deleted_line_group").await?;
        tx.commit().await?;
        Ok(result.rows_affected())
    }

    pub async fn replace_admin_line_group_lines(
        &self,
        line_group_id: Uuid,
        exit_endpoint_ids: Vec<Uuid>,
    ) -> Result<usize, DbError> {
        ensure_unique_uuids(&exit_endpoint_ids, "线路组成员重复")?;
        let mut tx = self.pool.begin().await?;
        ensure_line_group_exists_in_tx(&mut tx, line_group_id).await?;
        ensure_line_group_accepts_lines_in_tx(&mut tx, line_group_id).await?;
        if exit_endpoint_ids.is_empty() {
            sqlx::query("DELETE FROM line_group_exit_endpoints WHERE line_group_id = $1")
                .bind(line_group_id)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query(
                r#"
                DELETE FROM line_group_exit_endpoints
                WHERE line_group_id = $1 AND NOT (exit_endpoint_id = ANY($2))
                "#,
            )
            .bind(line_group_id)
            .bind(&exit_endpoint_ids)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("DELETE FROM line_group_lines WHERE line_group_id = $1")
            .bind(line_group_id)
            .execute(&mut *tx)
            .await?;
        for exit_endpoint_id in &exit_endpoint_ids {
            ensure_exit_endpoint_group_member_in_tx(&mut tx, *exit_endpoint_id).await?;
            sqlx::query(
                r#"
                INSERT INTO line_group_exit_endpoints (line_group_id, exit_endpoint_id)
                VALUES ($1, $2)
                ON CONFLICT (line_group_id, exit_endpoint_id) DO NOTHING
                "#,
            )
            .bind(line_group_id)
            .bind(exit_endpoint_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            r#"
            INSERT INTO line_group_binding_nodes (
                line_group_id, entry_exit_binding_id, position
            )
            SELECT $1, b.id, 100
            FROM access_entry_exit_bindings b
            WHERE b.exit_endpoint_id = ANY($2)
            ON CONFLICT (line_group_id, entry_exit_binding_id) DO NOTHING
            "#,
        )
        .bind(line_group_id)
        .bind(&exit_endpoint_ids)
        .execute(&mut *tx)
        .await?;
        sync_line_group_exit_pool_in_tx(&mut tx, line_group_id).await?;
        sqlx::query(
            r#"
            DELETE FROM user_exit_assignments uea
            WHERE EXISTS (
                SELECT 1
                FROM user_access_line_assignments a
                WHERE a.user_id = uea.user_id
                  AND a.access_line_id = uea.access_line_id
                  AND a.line_group_id = $1
            )
            "#,
        )
        .bind(line_group_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM user_access_line_assignments WHERE line_group_id = $1")
            .bind(line_group_id)
            .execute(&mut *tx)
            .await?;
        mark_all_nodes_dirty_in_tx(&mut tx, "admin_updated_line_group_lines").await?;
        tx.commit().await?;
        Ok(exit_endpoint_ids.len())
    }
}

pub(crate) async fn sync_line_group_exit_pool_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    line_group_id: Uuid,
) -> Result<Option<Uuid>, DbError> {
    let Some((name, country_code, current_exit_pool_id)) =
        sqlx::query_as::<_, (String, String, Option<Uuid>)>(
            "SELECT name, country_code, exit_pool_id FROM line_groups WHERE id = $1 FOR UPDATE",
        )
        .bind(line_group_id)
        .fetch_optional(&mut **tx)
        .await?
    else {
        return Err(DbError::InvalidInput("线路组不存在".to_string()));
    };

    let exit_endpoint_ids = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT exit_endpoint_id
        FROM line_group_exit_endpoints
        WHERE line_group_id = $1
        ORDER BY created_at, exit_endpoint_id
        "#,
    )
    .bind(line_group_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut exit_pool_id = current_exit_pool_id;
    if exit_pool_id.is_none() && !exit_endpoint_ids.is_empty() {
        let pool_name = required_admin_text(&format!("分组出口：{name}"), "线路组出口池名称", 128)?;
        let created_exit_pool_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO exit_pools (name, region_code, strategy, enabled, updated_at)
            VALUES ($1, $2, 'priority', TRUE, now())
            RETURNING id
            "#,
        )
        .bind(pool_name)
        .bind(if country_code.trim().is_empty() {
            "GLOBAL"
        } else {
            country_code.trim()
        })
        .fetch_one(&mut **tx)
        .await?;
        sqlx::query("UPDATE line_groups SET exit_pool_id = $2 WHERE id = $1")
            .bind(line_group_id)
            .bind(created_exit_pool_id)
            .execute(&mut **tx)
            .await?;
        exit_pool_id = Some(created_exit_pool_id);
    }

    let Some(exit_pool_id) = exit_pool_id else {
        return Ok(None);
    };

    // 历史「分组池」模型里，line_groups.exit_pool_id 可能与某条入口出口绑定 access_line 的
    // exit_pool_id 是同一个池（seed/迁移产物）。分组成员剪枝若无脑删掉不在新端点集合里的成员，
    // 会顺手删掉这个共享池里那条 binding 自己的出口成员，导致该 binding 的 access_line 没有出口、
    // 入口不渲染、订阅整单不可用（压测发现的 ordering BUG）。这里在剪枝时用 NOT EXISTS 守住任何
    // 被本池上「绑定线路」(access_lines.id 同主键命中 access_entry_exit_bindings) 引用的端点成员，
    // 让分组成员同步对 binding 自有出口只增不删，分组只负责维护自己引入的端点。
    if exit_endpoint_ids.is_empty() {
        sqlx::query(
            r#"
            DELETE FROM exit_pool_members m
            WHERE m.exit_pool_id = $1
              AND NOT EXISTS (
                  SELECT 1
                  FROM access_lines al
                  JOIN access_entry_exit_bindings b ON b.id = al.id
                  WHERE al.exit_pool_id = m.exit_pool_id
                    AND al.exit_endpoint_id = m.exit_endpoint_id
              )
            "#,
        )
        .bind(exit_pool_id)
        .execute(&mut **tx)
        .await?;
        return Ok(Some(exit_pool_id));
    }

    sqlx::query(
        r#"
        DELETE FROM exit_pool_members m
        WHERE m.exit_pool_id = $1
          AND NOT (m.exit_endpoint_id = ANY($2))
          AND NOT EXISTS (
              SELECT 1
              FROM access_lines al
              JOIN access_entry_exit_bindings b ON b.id = al.id
              WHERE al.exit_pool_id = m.exit_pool_id
                AND al.exit_endpoint_id = m.exit_endpoint_id
          )
        "#,
    )
    .bind(exit_pool_id)
    .bind(&exit_endpoint_ids)
    .execute(&mut **tx)
    .await?;

    for exit_endpoint_id in &exit_endpoint_ids {
        sqlx::query(
            r#"
            INSERT INTO exit_pool_members (
                exit_pool_id, exit_endpoint_id, weight, status,
                priority, allow_new_assignments, updated_at
            )
            VALUES ($1, $2, 100, 'healthy', 100, TRUE, now())
            ON CONFLICT (exit_pool_id, exit_endpoint_id) DO UPDATE SET
                weight = EXCLUDED.weight,
                status = EXCLUDED.status,
                priority = EXCLUDED.priority,
                allow_new_assignments = EXCLUDED.allow_new_assignments,
                updated_at = now()
            "#,
        )
        .bind(exit_pool_id)
        .bind(exit_endpoint_id)
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query(
        r#"
        UPDATE access_lines
        SET exit_pool_id = $2
        WHERE line_group_id = $1
          AND exit_endpoint_id IS NULL
        "#,
    )
    .bind(line_group_id)
    .bind(exit_pool_id)
    .execute(&mut **tx)
    .await?;

    Ok(Some(exit_pool_id))
}

async fn ensure_exit_endpoint_group_member_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_endpoint_id: Uuid,
) -> Result<(), DbError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM exit_endpoints e
            JOIN exit_resources r ON r.id = e.exit_resource_id
            WHERE e.id = $1
              AND e.enabled = TRUE
              AND r.enabled = TRUE
              AND e.outbound_type <> 'direct'
        )
        "#,
    )
    .bind(exit_endpoint_id)
    .fetch_one(&mut **tx)
    .await?;
    if !exists {
        return Err(DbError::InvalidAgentPayload(format!(
            "线路不存在或未启用: {exit_endpoint_id}"
        )));
    }
    Ok(())
}

fn normalize_line_group_level(value: Option<&str>) -> Result<String, DbError> {
    let _ = value;
    Ok("group".to_string())
}

fn normalized_dedicated_rules(rules: Vec<String>) -> Vec<String> {
    rules
        .into_iter()
        .map(|rule| rule.trim().to_string())
        .filter(|rule| !rule.is_empty())
        .collect()
}

async fn replace_line_group_rule_set_bindings_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    line_group_id: Uuid,
    bindings: Vec<LineGroupRuleSetBindingInput>,
) -> Result<(), DbError> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for binding in bindings {
        if !seen.insert(binding.rule_set_id) {
            return Err(DbError::InvalidInput(
                "同一线路分组不能重复绑定同一规则库".to_string(),
            ));
        }
        normalized.push(LineGroupRuleSetBindingInput {
            rule_set_id: binding.rule_set_id,
            position: binding.position.clamp(0, 1_000_000),
            enabled: binding.enabled,
        });
    }

    if !normalized.is_empty() {
        let ids = normalized
            .iter()
            .map(|binding| binding.rule_set_id)
            .collect::<Vec<_>>();
        let existing_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM subscription_rule_sets WHERE id = ANY($1)",
        )
        .bind(&ids)
        .fetch_one(&mut **tx)
        .await?;
        if existing_count != ids.len() as i64 {
            return Err(DbError::InvalidInput("规则库不存在".to_string()));
        }
    }

    sqlx::query("DELETE FROM line_group_rule_set_bindings WHERE line_group_id = $1")
        .bind(line_group_id)
        .execute(&mut **tx)
        .await?;

    for binding in normalized {
        sqlx::query(
            r#"
            INSERT INTO line_group_rule_set_bindings (
                line_group_id, rule_set_id, position, enabled
            )
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(line_group_id)
        .bind(binding.rule_set_id)
        .bind(binding.position)
        .bind(binding.enabled)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn validate_line_group_parent_in_tx(
    _tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    line_group_id: Option<Uuid>,
    group_level: &str,
    parent_group_id: Option<Uuid>,
) -> Result<(), DbError> {
    let _ = (line_group_id, group_level);
    if parent_group_id.is_some() {
        return Err(DbError::InvalidInput(
            "分组不再支持父级分组，请直接在线路分组中选择线路".to_string(),
        ));
    }
    Ok(())
}

async fn ensure_line_group_can_be_deleted_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    line_group_id: Uuid,
) -> Result<(), DbError> {
    let child_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM line_groups WHERE parent_group_id = $1")
            .bind(line_group_id)
            .fetch_one(&mut **tx)
            .await?;
    if child_count > 0 {
        return Err(DbError::InvalidInput(
            "请先删除或迁移该分组下的子分组".to_string(),
        ));
    }
    Ok(())
}

async fn ensure_line_group_accepts_lines_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    line_group_id: Uuid,
) -> Result<(), DbError> {
    sqlx::query_scalar::<_, String>("SELECT group_level FROM line_groups WHERE id = $1")
        .bind(line_group_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(())
}
