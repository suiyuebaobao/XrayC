//! 线路入口授权和具体出口选择 helper。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::rows::*;
use crate::*;
use uuid::Uuid;

pub(crate) async fn prune_unusable_exit_pool_lines_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_pool_ids: &[Uuid],
) -> Result<Vec<Uuid>, DbError> {
    reconcile_pool_lines_in_tx(tx, exit_pool_ids, true).await
}

pub(crate) async fn reconcile_binding_lines_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_pool_ids: &[Uuid],
) -> Result<Vec<Uuid>, DbError> {
    reconcile_pool_lines_in_tx(tx, exit_pool_ids, false).await
}

async fn reconcile_pool_lines_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exit_pool_ids: &[Uuid],
    include_legacy: bool,
) -> Result<Vec<Uuid>, DbError> {
    if exit_pool_ids.is_empty() {
        return Ok(Vec::new());
    }

    // 先清理指向不可新分配出口的旧记录，避免订阅和 agent 继续使用旧出口。
    sqlx::query(
        r#"
        DELETE FROM user_exit_assignments uea
        WHERE uea.exit_pool_id = ANY($1)
          AND NOT EXISTS (
              SELECT 1
              FROM exit_pool_members m
              JOIN exit_endpoints e ON e.id = m.exit_endpoint_id
              JOIN exit_resources r ON r.id = e.exit_resource_id
              WHERE m.exit_pool_id = uea.exit_pool_id
                AND m.exit_endpoint_id = uea.exit_endpoint_id
                AND m.status = 'healthy'
                AND m.allow_new_assignments = TRUE
                AND e.enabled = TRUE
                AND r.enabled = TRUE
          )
        "#,
    )
    .bind(exit_pool_ids)
    .execute(&mut **tx)
    .await?;

    // 一次计算最终状态，避免先启用再停用造成无效写入或跨节点反复置脏。
    let changed = sqlx::query_as::<_, (Uuid, Uuid, bool)>(
        r#"
        WITH desired AS (
            SELECT l.id, (CASE WHEN b.id IS NOT NULL THEN e.enabled AND b.enabled ELSE l.enabled END)
                AND EXISTS (
                    SELECT 1 FROM exit_pool_members m
                    JOIN exit_pools p ON p.id=m.exit_pool_id
                    JOIN exit_endpoints ee ON ee.id=m.exit_endpoint_id
                    JOIN exit_resources r ON r.id=ee.exit_resource_id
                    WHERE m.exit_pool_id=l.exit_pool_id AND p.enabled
                      AND m.status='healthy' AND m.allow_new_assignments AND ee.enabled AND r.enabled
                ) AS enabled
            FROM access_lines l
            LEFT JOIN access_entry_exit_bindings b ON b.id=l.id
            LEFT JOIN access_entries e ON e.id=b.access_entry_id
            WHERE l.exit_pool_id=ANY($1) AND ($2::boolean OR b.id IS NOT NULL)
        )
        UPDATE access_lines l SET enabled=d.enabled FROM desired d
        WHERE l.id=d.id AND l.enabled IS DISTINCT FROM d.enabled
        RETURNING l.id,l.access_node_id,l.enabled
        "#,
    ).bind(exit_pool_ids).bind(include_legacy).fetch_all(&mut **tx).await?;
    let disabled_line_ids: Vec<Uuid> = changed
        .iter()
        .filter(|(_, _, enabled)| !enabled)
        .map(|(id, _, _)| *id)
        .collect();
    for node in changed
        .iter()
        .map(|(_, node, _)| *node)
        .collect::<std::collections::HashSet<_>>()
    {
        super::dirty::mark_access_node_dirty_in_tx(tx, node, "runtime_binding_state_reconciled")
            .await?;
    }

    if !disabled_line_ids.is_empty() {
        sqlx::query("DELETE FROM user_access_line_assignments WHERE access_line_id = ANY($1)")
            .bind(&disabled_line_ids)
            .execute(&mut **tx)
            .await?;
        sqlx::query("DELETE FROM user_exit_assignments WHERE access_line_id = ANY($1)")
            .bind(&disabled_line_ids)
            .execute(&mut **tx)
            .await?;
    }

    Ok(disabled_line_ids)
}

pub(crate) fn choose_exit_endpoint_for_strategy(
    user_id: Uuid,
    access_line_id: Uuid,
    strategy: &str,
    candidates: &[EligibleExitEndpointRow],
) -> Option<Uuid> {
    if candidates.is_empty() {
        return None;
    }
    let mut ordered = candidates.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| right.weight.cmp(&left.weight))
            .then_with(|| {
                left.exit_endpoint_id
                    .as_u128()
                    .cmp(&right.exit_endpoint_id.as_u128())
            })
    });

    match strategy.trim().to_ascii_lowercase().as_str() {
        "hash" | "stable_hash" => {
            let index =
                ((user_id.as_u128() ^ access_line_id.as_u128()) % (ordered.len() as u128)) as usize;
            ordered.get(index).map(|item| item.exit_endpoint_id)
        }
        _ => ordered.first().map(|item| item.exit_endpoint_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: u128) -> EligibleExitEndpointRow {
        EligibleExitEndpointRow {
            exit_endpoint_id: Uuid::from_u128(id),
            weight: 100,
            priority: 100,
        }
    }

    #[test]
    fn priority_strategy_keeps_highest_priority_endpoint() {
        let candidates = vec![
            EligibleExitEndpointRow {
                exit_endpoint_id: Uuid::from_u128(1),
                weight: 100,
                priority: 10,
            },
            EligibleExitEndpointRow {
                exit_endpoint_id: Uuid::from_u128(2),
                weight: 100,
                priority: 20,
            },
        ];

        assert_eq!(
            choose_exit_endpoint_for_strategy(
                Uuid::from_u128(10),
                Uuid::from_u128(20),
                "priority",
                &candidates,
            ),
            Some(Uuid::from_u128(2))
        );
    }

    #[test]
    fn hash_strategy_stably_spreads_users_across_equal_candidates() {
        let candidates = vec![candidate(1), candidate(2)];
        let access_line_id = Uuid::from_u128(0);

        assert_eq!(
            choose_exit_endpoint_for_strategy(
                Uuid::from_u128(0),
                access_line_id,
                "hash",
                &candidates,
            ),
            Some(Uuid::from_u128(1))
        );
        assert_eq!(
            choose_exit_endpoint_for_strategy(
                Uuid::from_u128(1),
                access_line_id,
                "hash",
                &candidates,
            ),
            Some(Uuid::from_u128(2))
        );
        assert_eq!(
            choose_exit_endpoint_for_strategy(
                Uuid::from_u128(1),
                access_line_id,
                "hash",
                &candidates,
            ),
            Some(Uuid::from_u128(2))
        );
    }
}
