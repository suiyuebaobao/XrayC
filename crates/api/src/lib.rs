//! 本文件装配 XrayC V2 的 Axum HTTP API。
//! 具体 handler 已按路由域拆分到同目录模块。
//! 这里仅保留模块声明、共享导入和 Router 注册。
//! 路由路径、HTTP 方法、JSON 字段和状态码保持原实现。
//! cookie、token、限流和审计行为由子模块沿用。
//! 新增模块均为 crate 内可见，不改变外部公开 API。
//! 入口文件保持短小，便于审查和后续维护。
//! 文件前十行使用中文注释满足仓库约束。
//! 请勿在源码注释或日志中写入服务器敏感信息。
//! 只在本 crate 内组织 API 代码，不触碰 DB 或前端。

mod admin_backup;
#[cfg(test)]
mod admin_backup_tests;
mod admin_entries;
#[cfg(test)]
mod admin_entries_tests;
mod admin_exits;
mod admin_invites;
#[cfg(test)]
mod admin_invites_tests;
mod admin_lines;
mod admin_local_exits;
#[cfg(test)]
mod admin_local_exits_tests;
mod admin_nodes;
#[cfg(test)]
mod admin_nodes_reboot_tests;
#[cfg(test)]
mod admin_nodes_tests;
#[cfg(test)]
mod admin_nodes_tls_tests;
mod admin_plans;
#[cfg(test)]
mod admin_reset_password_tests;
#[cfg(test)]
mod admin_rule_sets_tests;
mod admin_settings;
#[cfg(test)]
mod admin_user_endpoint_tests;
mod admin_users;
mod agent;
mod agent_payload;
mod alipay;
mod audit;
mod auth;
mod billing;
mod deploy;
mod deploy_install;
mod deploy_install_job;
mod deploy_install_steps;
#[cfg(test)]
mod deploy_install_tests;
#[cfg(test)]
mod deploy_reinstall_tests;
#[cfg(test)]
mod deploy_tests;
mod deploy_validation;
#[cfg(test)]
mod deployment_task_admin_tests;
mod dto;
mod entry_cdn_detect;
mod epay;
mod guards;
mod operations;
#[cfg(test)]
mod operations_platform_metrics_tests;
mod payment;
mod public;
mod rate_limit;
mod remote_install;
#[cfg(test)]
mod removed_client_endpoint_tests;
mod request_defaults;
mod responses;
mod routing;
mod state;
mod user;
#[cfg(test)]
mod user_password_endpoint_tests;

use admin_backup::*;
use admin_entries::*;
use admin_exits::*;
use admin_invites::*;
use admin_lines::*;
use admin_local_exits::*;
use admin_nodes::*;
use admin_plans::*;
use admin_settings::*;
use admin_users::*;
use agent::*;
use agent_payload::*;
use audit::*;
use auth::*;
use billing::*;
use deploy::*;
use deploy_install_job::*;
use deploy_validation::*;
use operations::*;
use public::*;
use rate_limit::*;
use remote_install::*;
use routing::*;
use user::*;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs as std_fs;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;
use xrayc_core::{
    generate_clash_yaml, SubscriptionError, SubscriptionOptions, TrafficReport, TrafficReportResult,
};
use xrayc_db::{
    access_routing_json, exit_pools_json, heartbeat_json, operations_ledger_ranking_json,
    operations_summary_json, user_subscription_json, AddNodeDomainInput,
    AdminAccessEntryExitBindingInput, AdminAccessEntryInput, AdminAccessNodeInput,
    AdminAccessNodeUpdate, AdminExitEndpointInput, AdminExitEndpointUpdate, AdminExitResourceInput,
    AdminExitResourceUpdate, AdminLineGroupInput, AdminLocalExitLineInput,
    AdminLocalExitLineUpdate, AdminLocalExitLinesInput, AdminOrderFilters, AdminPlanInput,
    AdminPlanLineGroupInput, AdminPlanUpdate, AdminSubscriptionRuleSetInput, AdminUserCreate,
    AdminUserFilters, AdminUserUpdate, AgentTlsCertificateReport, AgentTlsRenewResult,
    AuditLogInput, CreateDeploymentTaskInput, DbError, DeploymentTaskReportInput,
    LineGroupRuleSetBindingInput, NodeRuntimeMetricsReport, PaymentCallbackInput, PgStore,
};

pub(crate) use dto::*;
pub(crate) use guards::*;
pub(crate) use request_defaults::*;
pub(crate) use responses::*;
pub use state::AppState;
pub(crate) use state::{AuthResponsePolicy, RateLimitScope};
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sub/:token", get(download_subscription))
        .route(
            "/api/deploy/artifacts/:artifact_name",
            get(download_deploy_artifact),
        )
        .route("/api/auth/security", get(auth_security))
        .route("/api/auth/captcha", get(auth_captcha))
        .route("/api/auth/email-code", post(send_email_code))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/refresh", post(refresh))
        .route("/api/auth/logout", post(logout))
        .route("/api/sales-landing", get(sales_landing))
        .route("/api/plans", get(list_plans))
        .route("/api/user/me", get(user_me))
        .route("/api/user/usage", get(user_usage))
        .route(
            "/api/user/invite-codes",
            get(user_invite_codes).post(create_user_invite_code),
        )
        .route("/api/orders", get(user_orders).post(create_order))
        .route("/api/payment/callback", post(payment_callback))
        .route("/api/payment/alipay/notify", post(alipay_notify))
        .route("/api/payment/epay/notify", post(epay_notify))
        .route("/api/payment/wechat/notify", post(wechat_notify))
        .route("/api/payment/channels", get(public_payment_channels))
        .route("/api/redeem", post(redeem_code))
        .route(
            "/api/user/subscription/token/reset",
            post(reset_user_subscription_token),
        )
        .route(
            "/api/user/password/send-code",
            post(send_change_password_code),
        )
        .route("/api/user/password", post(change_user_password))
        .route("/api/admin/users", get(admin_users).post(create_admin_user))
        .route(
            "/api/admin/users/batch-delete",
            post(batch_delete_admin_users),
        )
        .route(
            "/api/admin/users/:user_id",
            get(admin_user_detail)
                .patch(update_admin_user)
                .delete(delete_admin_user),
        )
        .route(
            "/api/admin/users/:user_id/subscription",
            get(admin_user_subscription),
        )
        .route(
            "/api/admin/users/:user_id/traffic-logs",
            get(admin_user_traffic_logs),
        )
        .route("/api/admin/users/:user_id/devices", get(admin_user_devices))
        .route(
            "/api/admin/users/:user_id/subscription-token/reset",
            post(reset_admin_user_subscription_token),
        )
        .route(
            "/api/admin/users/:user_id/reset-password",
            post(reset_admin_user_password),
        )
        .route("/api/admin/orders", get(admin_orders))
        .route(
            "/api/admin/redeem-codes",
            get(admin_redeem_codes).post(create_admin_redeem_codes),
        )
        .route(
            "/api/admin/invite-codes",
            get(admin_invite_codes).post(create_admin_invite_codes),
        )
        .route(
            "/api/admin/invite-codes/:code",
            delete(delete_admin_invite_code),
        )
        .route(
            "/api/admin/auth-security",
            get(admin_auth_security).put(update_admin_auth_security),
        )
        .route(
            "/api/admin/payment-settings",
            get(admin_payment_settings).put(update_admin_payment_settings),
        )
        .route(
            "/api/admin/subscription-settings",
            get(admin_subscription_settings).put(update_admin_subscription_settings),
        )
        .route(
            "/api/admin/subscription-rule-sets",
            get(list_subscription_rule_sets).post(create_subscription_rule_set),
        )
        .route(
            "/api/admin/subscription-rule-sets/:rule_set_id",
            put(update_subscription_rule_set).delete(delete_subscription_rule_set),
        )
        .route(
            "/api/admin/sales-landing",
            get(admin_sales_landing).put(update_admin_sales_landing),
        )
        .route("/api/admin/audit-logs", get(admin_audit_logs))
        .route(
            "/api/admin/backup/config",
            get(admin_backup_config).put(update_admin_backup_config),
        )
        .route(
            "/api/admin/backup/test-and-provision",
            post(admin_backup_test_and_provision),
        )
        .route("/api/admin/backup/run-now", post(admin_backup_run_now))
        .route("/api/admin/backup/state", get(admin_backup_state))
        .route(
            "/api/admin/access-nodes",
            get(admin_access_nodes).post(create_access_node),
        )
        .route(
            "/api/admin/access-nodes/batch-delete",
            post(batch_delete_access_nodes),
        )
        .route(
            "/api/admin/access-nodes/:access_node_id/tls/renew",
            post(renew_access_node_tls),
        )
        .route(
            "/api/admin/access-nodes/:access_node_id/reboot",
            post(reboot_access_node),
        )
        .route(
            "/api/admin/access-nodes/one-click-install",
            post(one_click_agent_install),
        )
        .route(
            "/api/admin/access-nodes/:access_node_id",
            put(update_access_node).delete(delete_access_node),
        )
        .route(
            "/api/admin/access-nodes/install-guide",
            post(agent_install_guide),
        )
        .route("/api/admin/deployment-tasks", get(deployment_tasks))
        .route(
            "/api/admin/deployment-tasks/:task_id",
            delete(delete_deployment_task),
        )
        .route(
            "/api/admin/deployment-tasks/:task_id/cancel",
            post(cancel_deployment_task),
        )
        .route(
            "/api/deployment-tasks/:task_id/report",
            post(report_deployment_task),
        )
        .route(
            "/api/admin/access-nodes/:access_node_id/local-exit-lines",
            get(list_local_exit_lines).post(create_local_exit_lines),
        )
        .route(
            "/api/admin/access-nodes/:access_node_id/local-exit-lines/:exit_endpoint_id",
            put(update_local_exit_line).delete(delete_local_exit_line),
        )
        .route("/api/admin/access-routing", get(access_routing))
        .route(
            "/api/admin/access-entries",
            get(list_access_entries).post(create_access_entry),
        )
        .route(
            "/api/admin/access-entries/:access_entry_id/exit-bindings",
            post(create_access_entry_exit_binding),
        )
        .route(
            "/api/admin/access-entries/:access_entry_id",
            put(update_access_entry).delete(delete_access_entry),
        )
        .route(
            "/api/admin/access-entry-exit-bindings",
            get(list_access_entry_exit_bindings),
        )
        .route(
            "/api/admin/access-entry-exit-bindings/:binding_id",
            put(update_access_entry_exit_binding).delete(delete_access_entry_exit_binding),
        )
        .route("/api/admin/access-lines", get(admin_access_lines))
        .route(
            "/api/admin/access-lines/:access_line_id/sessions",
            get(access_line_sessions),
        )
        .route(
            "/api/admin/access-lines/:access_line_id/metrics",
            get(access_line_metrics),
        )
        .route(
            "/api/admin/exit-resources",
            get(admin_exit_resources).post(create_exit_resource),
        )
        .route(
            "/api/admin/exit-resources/:exit_resource_id",
            put(update_exit_resource),
        )
        .route(
            "/api/admin/exit-endpoints",
            get(admin_exit_endpoints).post(create_exit_endpoint),
        )
        .route(
            "/api/admin/exit-endpoints/:exit_endpoint_id",
            put(update_exit_endpoint).delete(delete_exit_endpoint),
        )
        .route(
            "/api/admin/exit-endpoints/:exit_endpoint_id/probe",
            post(trigger_exit_endpoint_probe),
        )
        .route("/api/admin/exit-pools", get(exit_pools))
        .route("/api/admin/line-groups", post(create_line_group))
        .route(
            "/api/admin/line-groups/:line_group_id",
            put(update_line_group).delete(delete_line_group),
        )
        .route(
            "/api/admin/line-groups/:line_group_id/lines",
            put(replace_line_group_lines),
        )
        .route(
            "/api/admin/line-groups/:line_group_id/binding-nodes",
            put(replace_line_group_binding_nodes),
        )
        .route("/api/admin/plans", get(admin_plans).post(create_admin_plan))
        .route(
            "/api/admin/plans/:plan_id",
            patch(update_admin_plan)
                .put(update_admin_plan)
                .delete(delete_admin_plan),
        )
        .route(
            "/api/admin/plans/:plan_id/line-groups",
            put(replace_plan_line_groups),
        )
        .route("/api/user/subscription", get(user_subscription))
        .route(
            "/api/admin/access-operations/summary",
            get(operations_summary),
        )
        .route(
            "/api/admin/access-operations/ledger-ranking",
            get(operations_ledger_ranking),
        )
        .route(
            "/api/admin/access-operations/settings",
            get(access_operations_settings).put(update_access_operations_settings),
        )
        .route(
            "/api/admin/access-operations/probe",
            post(trigger_access_operations_probe),
        )
        .route(
            "/api/admin/access-operations/platform-metrics",
            get(platform_metrics),
        )
        .route("/api/admin/monitor/node-traffic", get(node_traffic_summary))
        .route(
            "/api/admin/monitor/node-traffic/:node_id/trend",
            get(node_traffic_trend),
        )
        .route("/api/agent/access/heartbeat", post(agent_heartbeat))
        .route("/api/agent/access/config-result", post(agent_config_result))
        .route("/api/agent/access/traffic", post(agent_traffic))
        .route("/api/agent/access/metrics", post(agent_metrics))
        .route("/api/agent/access/sessions", post(agent_sessions))
        .route("/api/agent/access/probes", post(agent_probes))
        .with_state(Arc::new(state))
}
