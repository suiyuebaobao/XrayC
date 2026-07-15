//! 本模块收纳后台和订单相关的 JSON 转换函数。
//! 这些函数只把已查询出的行结构转换成响应 JSON。
//! SQL 查询和 PgStore 方法仍保留在 crate root 中。
//! 管理员审计摘要按数据库保存内容展示，不在本层二次改写。
//! 流量展示复用 helpers 中的字节换算函数。
//! 支付回调响应只组装返回体，不修改订单状态。
//! 行结构仍定义在 crate root，避免打散 SQL 映射上下文。
//! 新增后台列表响应转换应优先放在本模块。
//! 本模块不得执行数据库访问或事务操作。
//! 文件行数保持低于 500 行，便于后续拆分。

use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};

use super::{
    AdminInviteCodeRow, AdminPlanRow, AdminUserListRow, AuditLogListRow, LineLedgerTotalsRow,
    OperationsLedgerRankingRow, OperationsLedgerRankingTotalsRow, OrderListRow, PaymentOrderRow,
    RedeemCodeRow, UserInviteCodeRow,
};
use crate::helpers::bytes_to_gb;

pub(super) fn admin_collection_json(field: &str, items: Value) -> Value {
    let mut payload = Map::new();
    payload.insert("items".to_string(), items.clone());
    payload.insert(field.to_string(), items);
    Value::Object(payload)
}

pub(super) fn admin_audit_log_json(row: AuditLogListRow) -> Value {
    let request_summary = row.request_summary;
    json!({
        "id": row.id,
        "created_at": row.created_at,
        "actor_email": row.actor_email,
        "actor": {
            "email": row.actor_email,
        },
        "action": row.action,
        "resource_type": row.resource_type,
        "resource_id": row.resource_id,
        "resource": {
            "type": row.resource_type,
            "id": row.resource_id,
        },
        "result": row.result,
        "request_summary": request_summary,
        "summary": request_summary,
    })
}

pub(super) fn ledger_totals_json(row: &LineLedgerTotalsRow) -> Value {
    json!({
        "ledger_count": row.ledger_count.max(0),
        "delta_uplink": row.delta_uplink.max(0),
        "delta_downlink": row.delta_downlink.max(0),
        "delta_total": row.delta_total.max(0),
        "real_bytes": row.delta_total.max(0),
        "billed_uplink": row.billed_uplink.max(0),
        "billed_downlink": row.billed_downlink.max(0),
        "billed_bytes": row.billed_bytes.max(0)
    })
}

pub(super) fn operations_ledger_ranking_row_json(
    rank: usize,
    row: &OperationsLedgerRankingRow,
) -> Value {
    json!({
        "rank": rank,
        "access_line_id": row.access_line_id,
        "access_line_name": row.access_line_name,
        "access_node_name": row.access_node_name,
        "region_code": row.region_code,
        "listen_host": row.listen_host,
        "listen_port": row.listen_port.clamp(0, u16::MAX as i32),
        "ledger_count": row.ledger_count.max(0),
        "delta_uplink": row.delta_uplink.max(0),
        "delta_downlink": row.delta_downlink.max(0),
        "delta_total": row.delta_total.max(0),
        "real_bytes": row.delta_total.max(0),
        "billed_uplink": row.billed_uplink.max(0),
        "billed_downlink": row.billed_downlink.max(0),
        "billed_bytes": row.billed_bytes.max(0),
        "latest_collected_at": row.latest_collected_at
    })
}

pub(super) fn operations_ledger_ranking_totals_json(
    row: &OperationsLedgerRankingTotalsRow,
) -> Value {
    json!({
        "access_line_count": row.access_line_count.max(0),
        "ledger_count": row.ledger_count.max(0),
        "delta_uplink": row.delta_uplink.max(0),
        "delta_downlink": row.delta_downlink.max(0),
        "delta_total": row.delta_total.max(0),
        "real_bytes": row.delta_total.max(0),
        "billed_uplink": row.billed_uplink.max(0),
        "billed_downlink": row.billed_downlink.max(0),
        "billed_bytes": row.billed_bytes.max(0),
        "latest_collected_at": row.latest_collected_at
    })
}

pub(super) fn admin_plan_json(row: AdminPlanRow, line_groups: Vec<Value>) -> Value {
    json!({
        "id": row.id,
        "name": row.name,
        "is_default": row.is_default,
        "traffic_limit_bytes": row.traffic_limit_bytes,
        "rate_limit_bps": row.rate_limit_bps,
        "rate_limit_up_bps": row.rate_limit_up_bps,
        "rate_limit_down_bps": row.rate_limit_down_bps,
        "billing_multiplier": row.billing_multiplier,
        "enabled": row.enabled,
        "price_cents": row.price_cents,
        "currency": row.currency,
        "duration_days": row.duration_days,
        "sort_weight": row.sort_weight,
        "is_deleted": row.is_deleted,
        "created_at": row.created_at,
        "default_line_group_id": row.default_line_group_id,
        "line_groups": line_groups
    })
}

pub(super) fn admin_user_json(row: AdminUserListRow) -> Value {
    let remaining_bytes = (row.limit_bytes - row.used_bytes).max(0);
    let effective_rate_limit_bps = row
        .user_rate_limit_bps
        .unwrap_or(row.plan_rate_limit_bps)
        .max(0);
    json!({
        "id": row.id,
        "email": row.email,
        "account": row.email,
        "name": if row.display_name.is_empty() { row.email.clone() } else { row.display_name },
        "disabled": row.disabled,
        "is_admin": row.is_admin,
        "status": if row.disabled { "disabled" } else { "active" },
        "role": if row.is_admin { "admin" } else { "user" },
        "created_at": row.created_at,
        "plan_id": row.plan_id,
        "plan_name": row.plan_name.unwrap_or_else(|| "未开通".to_string()),
        "subscription_status": if row.subscription_active.unwrap_or(false) { "active" } else { "" },
        "subscription_expires_at": row.expires_at,
        "subscription_active": row.subscription_active.unwrap_or(false),
        "expires_at": row.expires_at,
        "traffic_gb": {
            "used_gb": bytes_to_gb(row.used_bytes.max(0) as u64),
            "total_gb": bytes_to_gb(row.limit_bytes.max(0) as u64)
        },
        "remaining_bytes": remaining_bytes,
        "remaining_gb": bytes_to_gb(remaining_bytes as u64),
        "plan_rate_limit_bps": row.plan_rate_limit_bps.max(0),
        "user_rate_limit_bps": row.user_rate_limit_bps.map(|value| value.max(0)),
        "user_rate_limit_up_bps": row.user_rate_limit_up_bps,
        "user_rate_limit_down_bps": row.user_rate_limit_down_bps,
        "effective_rate_limit_bps": effective_rate_limit_bps
    })
}

pub(super) fn order_json(row: OrderListRow) -> Value {
    json!({
        "id": row.id,
        "order_no": row.order_no,
        "user_id": row.user_id,
        "user_email": row.user_email,
        "plan_name": row.plan_name,
        "amount_cents": row.amount_cents,
        "amount": (row.amount_cents as f64) / 100.0,
        "currency": row.currency,
        "status": row.status,
        "payment_address": row.payment_address,
        "expires_at": row.expires_at,
        "duration_days": row.duration_days,
        "paid_at": row.paid_at,
        "created_at": row.created_at
    })
}

pub(super) fn payment_callback_response(
    order_no: &str,
    order: &PaymentOrderRow,
    idempotent: bool,
    next_expires_at: Option<DateTime<Utc>>,
) -> Value {
    json!({
        "order_no": order_no,
        "order_id": order.id,
        "user_id": order.user_id,
        "plan_id": order.plan_id,
        "plan_name": order.plan_name,
        "status": "paid",
        "idempotent": idempotent,
        "expires_at": next_expires_at
    })
}

pub(super) fn is_successful_payment_status(status: &str, confirmations: Option<i32>) -> bool {
    if !matches!(status, "paid" | "confirmed" | "success" | "completed") {
        return false;
    }
    let min_confirmations = std::env::var("PAYMENT_MIN_CONFIRMATIONS")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(1)
        .max(0);
    confirmations.unwrap_or_default() >= min_confirmations
}

pub(super) fn redeem_code_json(row: RedeemCodeRow) -> Value {
    json!({
        "code": row.code,
        "plan_id": row.plan_id,
        "plan_name": row.plan_name,
        "traffic_bytes": row.traffic_bytes,
        "duration_days": row.duration_days,
        "is_used": row.is_used,
        "used_by_user_id": row.used_by_user_id,
        "used_by_email": row.used_by_email,
        "used_at": row.used_at,
        "expires_at": row.expires_at,
        "created_at": row.created_at
    })
}

pub(super) fn user_invite_code_json(row: UserInviteCodeRow) -> Value {
    json!({
        "code": row.code,
        "is_used": row.is_used,
        "used_by_user_id": row.used_by_user_id,
        "used_by_email": row.used_by_email_snapshot,
        "used_at": row.used_at,
        "created_at": row.created_at
    })
}

pub(super) fn admin_invite_code_json(row: AdminInviteCodeRow) -> Value {
    json!({
        "code": row.code,
        "inviter_user_id": row.inviter_user_id,
        "inviter_email": row.inviter_email_snapshot,
        "is_used": row.is_used,
        "used_by_user_id": row.used_by_user_id,
        "used_by_email": row.used_by_email_snapshot,
        "used_at": row.used_at,
        "created_at": row.created_at
    })
}
