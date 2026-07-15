//! 订单创建、列表和支付回调处理。
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
    pub async fn user_orders_json(&self, user_id: Uuid) -> Result<Value, DbError> {
        self.orders_json(Some(user_id)).await
    }

    pub async fn admin_orders_json(&self) -> Result<Value, DbError> {
        self.orders_json(None).await
    }

    pub async fn admin_orders_filtered_json(
        &self,
        filters: AdminOrderFilters,
    ) -> Result<Value, DbError> {
        let page = filters.page.unwrap_or(1).max(1);
        let page_size = filters
            .page_size
            .unwrap_or(50)
            .clamp(1, ADMIN_ORDER_MAX_PAGE_SIZE);
        let offset = page.saturating_sub(1).saturating_mul(page_size);
        let status = normalized_filter(filters.status);
        let user = normalized_filter(filters.user);
        let email = normalized_filter(filters.email);
        let keyword = normalized_filter(filters.keyword);
        let order_no = normalized_filter(filters.order_no);

        let total = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::BIGINT
            FROM orders o
            JOIN users u ON u.id = o.user_id
            JOIN plans p ON p.id = o.plan_id
            WHERE ($1::text IS NULL OR lower(o.status) = lower($1))
              AND ($2::text IS NULL OR u.email ILIKE '%' || $2 || '%')
              AND ($3::text IS NULL OR u.email ILIKE '%' || $3 || '%')
              AND ($4::text IS NULL OR u.email ILIKE '%' || $4 || '%' OR o.order_no ILIKE '%' || $4 || '%')
              AND ($5::text IS NULL OR o.order_no ILIKE '%' || $5 || '%')
            "#,
        )
        .bind(&status)
        .bind(&user)
        .bind(&email)
        .bind(&keyword)
        .bind(&order_no)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, OrderListRow>(
            r#"
            SELECT o.id,
                   o.order_no,
                   o.user_id,
                   u.email AS user_email,
                   p.name AS plan_name,
                   o.amount_cents,
                   o.currency,
                   o.status,
                   o.payment_address,
                   o.expires_at,
                   o.paid_at,
                   o.created_at,
                   p.duration_days
            FROM orders o
            JOIN users u ON u.id = o.user_id
            JOIN plans p ON p.id = o.plan_id
            WHERE ($1::text IS NULL OR lower(o.status) = lower($1))
              AND ($2::text IS NULL OR u.email ILIKE '%' || $2 || '%')
              AND ($3::text IS NULL OR u.email ILIKE '%' || $3 || '%')
              AND ($4::text IS NULL OR u.email ILIKE '%' || $4 || '%' OR o.order_no ILIKE '%' || $4 || '%')
              AND ($5::text IS NULL OR o.order_no ILIKE '%' || $5 || '%')
            ORDER BY o.created_at DESC, o.id DESC
            LIMIT $6 OFFSET $7
            "#,
        )
        .bind(&status)
        .bind(&user)
        .bind(&email)
        .bind(&keyword)
        .bind(&order_no)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(json!({
            "items": rows.into_iter().map(order_json).collect::<Vec<_>>(),
            "total": total.max(0),
            "page": page,
            "page_size": page_size
        }))
    }

    pub(crate) async fn orders_json(&self, user_id: Option<Uuid>) -> Result<Value, DbError> {
        let rows = sqlx::query_as::<_, OrderListRow>(
            r#"
            SELECT o.id,
                   o.order_no,
                   o.user_id,
                   u.email AS user_email,
                   p.name AS plan_name,
                   o.amount_cents,
                   o.currency,
                   o.status,
                   o.payment_address,
                   o.expires_at,
                   o.paid_at,
                   o.created_at,
                   p.duration_days
            FROM orders o
            JOIN users u ON u.id = o.user_id
            JOIN plans p ON p.id = o.plan_id
            WHERE ($1::uuid IS NULL OR o.user_id = $1)
            ORDER BY o.created_at DESC
            LIMIT 500
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "items": rows.into_iter().map(order_json).collect::<Vec<_>>()
        }))
    }

    pub async fn create_order_json(&self, user_id: Uuid, plan_id: Uuid) -> Result<Value, DbError> {
        let plan = sqlx::query_as::<_, PlanCheckoutRow>(
            r#"
            SELECT id, name, price_cents, currency, duration_days
            FROM plans
            WHERE id = $1 AND enabled = TRUE AND is_deleted = FALSE
            "#,
        )
        .bind(plan_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::DefaultPlanNotFound)?;
        let order_no = format!("ORD{}", Uuid::new_v4().simple());
        let payment_address = std::env::var("PAYMENT_RECEIVE_ADDRESS")
            .unwrap_or_else(|_| "xrayc-demo-payment-address".to_string());
        let row = sqlx::query_as::<_, OrderListRow>(
            r#"
            INSERT INTO orders (
                order_no, user_id, plan_id, amount_cents, currency, payment_address,
                status, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, 'pending', now() + interval '30 minutes')
            RETURNING id,
                      order_no,
                      user_id,
                      (SELECT email FROM users WHERE id = $2) AS user_email,
                      $7::text AS plan_name,
                      amount_cents,
                      currency,
                      status,
                      payment_address,
                      expires_at,
                      paid_at,
                      created_at,
                      $8::int AS duration_days
            "#,
        )
        .bind(order_no)
        .bind(user_id)
        .bind(plan.id)
        .bind(plan.price_cents)
        .bind(&plan.currency)
        .bind(payment_address)
        .bind(plan.name)
        .bind(plan.duration_days)
        .fetch_one(&self.pool)
        .await?;
        Ok(order_json(row))
    }

    pub async fn apply_payment_callback_json(
        &self,
        input: PaymentCallbackInput,
    ) -> Result<Value, DbError> {
        let order_no = input.order_no.trim();
        let tx_id = input.tx_id.trim();
        if order_no.is_empty() || order_no.len() > 128 {
            return Err(DbError::InvalidPaymentCallback("订单号无效".to_string()));
        }
        if tx_id.is_empty() || tx_id.len() > 256 {
            return Err(DbError::InvalidPaymentCallback(
                "交易流水号无效".to_string(),
            ));
        }
        if input.amount_cents < 0 {
            return Err(DbError::InvalidPaymentCallback("支付金额无效".to_string()));
        }
        let payment_status = input.status.trim().to_ascii_lowercase();
        if !is_successful_payment_status(&payment_status, input.confirmations) {
            return Err(DbError::InvalidPaymentCallback(
                "支付状态未确认".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await?;
        let order = sqlx::query_as::<_, PaymentOrderRow>(
            r#"
            SELECT o.id,
                   o.user_id,
                   o.plan_id,
                   p.name AS plan_name,
                   o.amount_cents,
                   o.currency,
                   o.status,
                   o.payment_address,
                   o.expires_at,
                   p.traffic_limit_bytes,
                   p.duration_days
            FROM orders o
            JOIN plans p ON p.id = o.plan_id
            WHERE o.order_no = $1
            FOR UPDATE OF o
            "#,
        )
        .bind(order_no)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::InvalidPaymentCallback("订单不存在".to_string()))?;

        if order.status == "paid" {
            let existing_order_id = sqlx::query_scalar::<_, Uuid>(
                "SELECT order_id FROM payment_records WHERE tx_id = $1",
            )
            .bind(tx_id)
            .fetch_optional(&mut *tx)
            .await?;
            if existing_order_id == Some(order.id) {
                tx.commit().await?;
                return Ok(payment_callback_response(order_no, &order, true, None));
            }
            return Err(DbError::InvalidPaymentCallback("订单已支付".to_string()));
        }

        if order.status != "pending" {
            return Err(DbError::InvalidPaymentCallback(format!(
                "订单状态不可支付: {}",
                order.status
            )));
        }
        if order.expires_at <= Utc::now() {
            return Err(DbError::InvalidPaymentCallback("订单已过期".to_string()));
        }
        if !order.currency.eq_ignore_ascii_case(input.currency.trim()) {
            return Err(DbError::InvalidPaymentCallback(
                "支付币种不匹配".to_string(),
            ));
        }
        if input.amount_cents != order.amount_cents {
            return Err(DbError::InvalidPaymentCallback(
                "支付金额不匹配".to_string(),
            ));
        }
        let callback_payment_address = input.payment_address.as_deref().unwrap_or_default().trim();
        if callback_payment_address.is_empty() {
            return Err(DbError::InvalidPaymentCallback("收款地址缺失".to_string()));
        }
        if callback_payment_address != order.payment_address.trim() {
            return Err(DbError::InvalidPaymentCallback(
                "收款地址不匹配".to_string(),
            ));
        }

        if let Some(existing_order_id) =
            sqlx::query_scalar::<_, Uuid>("SELECT order_id FROM payment_records WHERE tx_id = $1")
                .bind(tx_id)
                .fetch_optional(&mut *tx)
                .await?
        {
            if existing_order_id == order.id {
                tx.commit().await?;
                return Ok(payment_callback_response(order_no, &order, true, None));
            }
            return Err(DbError::InvalidPaymentCallback(
                "交易流水号已被其他订单使用".to_string(),
            ));
        }

        let confirmed_at = input.paid_at.unwrap_or_else(Utc::now);
        sqlx::query(
            r#"
            INSERT INTO payment_records (
                order_id, tx_id, amount_cents, currency, raw_payload, confirmed_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(order.id)
        .bind(tx_id)
        .bind(input.amount_cents)
        .bind(input.currency.trim().to_ascii_uppercase())
        .bind(input.raw_payload)
        .bind(confirmed_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE orders
            SET status = 'paid', paid_at = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(order.id)
        .bind(confirmed_at)
        .execute(&mut *tx)
        .await?;

        let base_expires_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
            "SELECT expires_at FROM user_subscriptions WHERE user_id = $1 FOR UPDATE",
        )
        .bind(order.user_id)
        .fetch_optional(&mut *tx)
        .await?
        .flatten()
        .unwrap_or_else(Utc::now);
        let next_expires_at = std::cmp::max(base_expires_at, Utc::now())
            + Duration::days(order.duration_days.max(0) as i64);
        sqlx::query(
            r#"
            INSERT INTO user_subscriptions (
                user_id, plan_id, active, expires_at,
                used_bytes, limit_bytes
            )
            VALUES ($1, $2, TRUE, $3, 0, $4)
            ON CONFLICT (user_id) DO UPDATE SET
                plan_id = EXCLUDED.plan_id,
                active = TRUE,
                expires_at = $3,
                limit_bytes = user_subscriptions.limit_bytes + EXCLUDED.limit_bytes,
                updated_at = now()
            "#,
        )
        .bind(order.user_id)
        .bind(order.plan_id)
        .bind(next_expires_at)
        .bind(order.traffic_limit_bytes)
        .execute(&mut *tx)
        .await?;

        // 支付回调可能作用于历史异常用户，因此成功入账时必须保证订阅 token 存在。
        let subscription_token = format!("sub-{}", Uuid::new_v4().simple());
        sqlx::query(
            r#"
            INSERT INTO subscription_tokens (
                token, token_hash, user_id, revoked_at, expires_at
            )
            VALUES ($1, $2, $3, NULL, $4)
            ON CONFLICT (user_id) DO NOTHING
            "#,
        )
        .bind(&subscription_token)
        .bind(agent_token_hash(&subscription_token))
        .bind(order.user_id)
        .bind(subscription_token_expires_at())
        .execute(&mut *tx)
        .await?;

        mark_all_nodes_dirty_in_tx(&mut tx, "payment_callback_paid").await?;
        tx.commit().await?;
        Ok(payment_callback_response(
            order_no,
            &order,
            false,
            Some(next_expires_at),
        ))
    }
}
