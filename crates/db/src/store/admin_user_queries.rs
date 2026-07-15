//! 后台用户查询模块。
//! 本模块只负责管理员用户列表和单用户详情读取。
//! 创建、编辑、删除仍保留在 users 模块，避免事务逻辑分散。
//! 查询支持分页、关键词、邮箱、状态、角色和套餐过滤。
//! 返回 JSON 字段继续复用统一后台用户读模型。
//! SQL 使用 QueryBuilder 绑定变量，避免拼接用户输入。
//! 页大小在服务端限制，防止后台列表一次拉取过大。
//! 单用户详情和列表保持同一字段结构，方便前端复用。
//! 注释必须使用中文，满足仓库注释规则。
//! 本文件不记录任何服务器、密钥或真实用户凭据。

use super::rows::*;
use crate::*;
use serde_json::json;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

impl PgStore {
    pub async fn admin_users_json(&self, filters: AdminUserFilters) -> Result<Value, DbError> {
        let normalized = NormalizedAdminUserFilters::from(filters);
        let mut count_query = QueryBuilder::<Postgres>::new(
            r#"
            SELECT COUNT(*)::bigint
            FROM users u
            LEFT JOIN user_subscriptions s ON s.user_id = u.id
            LEFT JOIN plans p ON p.id = s.plan_id
            "#,
        );
        append_admin_user_filters(&mut count_query, &normalized);
        let total = count_query
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await?;

        let mut list_query = QueryBuilder::<Postgres>::new(ADMIN_USER_SELECT);
        append_admin_user_filters(&mut list_query, &normalized);
        list_query.push(" ORDER BY u.created_at DESC LIMIT ");
        list_query.push_bind(normalized.page_size);
        list_query.push(" OFFSET ");
        list_query.push_bind((normalized.page - 1) * normalized.page_size);
        let rows = list_query
            .build_query_as::<AdminUserListRow>()
            .fetch_all(&self.pool)
            .await?;
        let items = rows.into_iter().map(admin_user_json).collect::<Vec<_>>();
        Ok(json!({
            "items": items,
            "total": total,
            "page": normalized.page,
            "page_size": normalized.page_size
        }))
    }

    pub async fn admin_user_json(&self, user_id: Uuid) -> Result<Value, DbError> {
        let row = sqlx::query_as::<_, AdminUserListRow>(
            r#"
            SELECT u.id,
                   u.email,
                   u.display_name,
                   u.disabled,
                   u.is_admin,
                   u.created_at,
                   s.plan_id,
                   p.name AS plan_name,
                   s.active AS subscription_active,
                   s.expires_at,
                   COALESCE(s.used_bytes, 0)::bigint AS used_bytes,
                   COALESCE(s.limit_bytes, 0)::bigint AS limit_bytes,
                   COALESCE(p.rate_limit_bps, 0)::bigint AS plan_rate_limit_bps,
                   u.rate_limit_bps AS user_rate_limit_bps,
                   u.rate_limit_up_bps AS user_rate_limit_up_bps,
                   u.rate_limit_down_bps AS user_rate_limit_down_bps
            FROM users u
            LEFT JOIN user_subscriptions s ON s.user_id = u.id
            LEFT JOIN plans p ON p.id = s.plan_id
            WHERE u.id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::UserNotFound)?;
        Ok(admin_user_json(row))
    }
}

const ADMIN_USER_SELECT: &str = r#"
    SELECT u.id,
           u.email,
           u.display_name,
           u.disabled,
           u.is_admin,
           u.created_at,
           s.plan_id,
           p.name AS plan_name,
           s.active AS subscription_active,
           s.expires_at,
           COALESCE(s.used_bytes, 0)::bigint AS used_bytes,
           COALESCE(s.limit_bytes, 0)::bigint AS limit_bytes,
           COALESCE(p.rate_limit_bps, 0)::bigint AS plan_rate_limit_bps,
           u.rate_limit_bps AS user_rate_limit_bps,
           u.rate_limit_up_bps AS user_rate_limit_up_bps,
           u.rate_limit_down_bps AS user_rate_limit_down_bps
    FROM users u
    LEFT JOIN user_subscriptions s ON s.user_id = u.id
    LEFT JOIN plans p ON p.id = s.plan_id
    "#;

struct NormalizedAdminUserFilters {
    page: i64,
    page_size: i64,
    keyword: String,
    email: String,
    status: String,
    role: String,
    plan_id: Option<Uuid>,
}

impl From<AdminUserFilters> for NormalizedAdminUserFilters {
    fn from(filters: AdminUserFilters) -> Self {
        Self {
            page: filters.page.unwrap_or(1).max(1),
            page_size: filters.page_size.unwrap_or(20).clamp(1, 200),
            keyword: normalized_filter(filters.keyword)
                .unwrap_or_default()
                .to_ascii_lowercase(),
            email: normalized_filter(filters.email)
                .unwrap_or_default()
                .to_ascii_lowercase(),
            status: normalized_filter(filters.status)
                .unwrap_or_default()
                .to_ascii_lowercase(),
            role: normalized_filter(filters.role)
                .unwrap_or_default()
                .to_ascii_lowercase(),
            plan_id: filters.plan_id,
        }
    }
}

fn append_admin_user_filters(
    query: &mut QueryBuilder<'_, Postgres>,
    filters: &NormalizedAdminUserFilters,
) {
    let mut has_where = false;
    append_text_filter(
        query,
        &mut has_where,
        "LOWER(u.email) LIKE ",
        &filters.email,
    );
    append_keyword_filter(query, &mut has_where, filters);
    match filters.status.as_str() {
        "active" | "enabled" => append_raw_filter(query, &mut has_where, "u.disabled = FALSE"),
        "disabled" => append_raw_filter(query, &mut has_where, "u.disabled = TRUE"),
        _ => {}
    }
    match filters.role.as_str() {
        "admin" => append_raw_filter(query, &mut has_where, "u.is_admin = TRUE"),
        "user" => append_raw_filter(query, &mut has_where, "u.is_admin = FALSE"),
        _ => {}
    }
    if let Some(plan_id) = filters.plan_id {
        append_where(query, &mut has_where);
        query.push("s.plan_id = ");
        query.push_bind(plan_id);
    }
}

fn append_keyword_filter(
    query: &mut QueryBuilder<'_, Postgres>,
    has_where: &mut bool,
    filters: &NormalizedAdminUserFilters,
) {
    if filters.keyword.is_empty() {
        return;
    }
    append_where(query, has_where);
    let pattern = format!("%{}%", filters.keyword);
    query.push("(LOWER(u.email) LIKE ");
    query.push_bind(pattern.clone());
    query.push(" OR LOWER(u.display_name) LIKE ");
    query.push_bind(pattern.clone());
    query.push(" OR LOWER(COALESCE(p.name, '')) LIKE ");
    query.push_bind(pattern);
    query.push(")");
}

fn append_text_filter(
    query: &mut QueryBuilder<'_, Postgres>,
    has_where: &mut bool,
    clause: &str,
    value: &str,
) {
    if value.is_empty() {
        return;
    }
    append_where(query, has_where);
    query.push(clause);
    query.push_bind(format!("%{value}%"));
}

fn append_raw_filter(query: &mut QueryBuilder<'_, Postgres>, has_where: &mut bool, clause: &str) {
    append_where(query, has_where);
    query.push(clause);
}

fn append_where(query: &mut QueryBuilder<'_, Postgres>, has_where: &mut bool) {
    if *has_where {
        query.push(" AND ");
    } else {
        query.push(" WHERE ");
        *has_where = true;
    }
}
