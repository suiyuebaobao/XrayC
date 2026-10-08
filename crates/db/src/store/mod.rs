//! PgStore 业务域拆分聚合模块。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
pub(crate) mod admin_user_queries;
pub(crate) mod agent;
mod heartbeat_config;
pub(crate) mod agent_kernel_reboot;
pub(crate) mod agent_metrics;
pub(crate) mod agent_session_helpers;
pub(crate) mod agent_sessions;
pub(crate) mod assignments_access;
pub(crate) mod assignments_exit;
pub(crate) mod auth_accounts;
pub(crate) mod auth_challenges;
pub(crate) mod auth_sessions;
pub(crate) mod bootstrap;
pub(crate) mod core;
pub(crate) mod deployment_task_progress;
mod deployment_task_reports;
pub(crate) mod deployment_task_reinstall;
pub(crate) mod deployment_tasks;
pub(crate) mod dirty;
pub(crate) mod existence;
pub(crate) mod json_util;
pub(crate) mod line_binding;
pub(crate) mod load;
pub(crate) mod local_access_defaults;
pub(crate) mod maintenance;
mod maintenance_jobs;
pub(crate) mod maintenance_probe_tasks;
pub(crate) mod maintenance_usage_rollups;
pub(crate) mod node_metrics;
pub(crate) mod node_traffic;
pub(crate) mod operations;
pub(crate) mod orders;
pub(crate) mod plans;
pub(crate) mod probes;
pub(crate) mod redeem;
pub(crate) mod routing_access_entries;
pub(crate) mod routing_access_entries_read;
pub(crate) mod routing_access_entries_rows;
pub(crate) mod routing_access_entry_updates;
pub(crate) mod routing_access_line_cleanup;
pub(crate) mod routing_access_nodes_write;
pub(crate) mod routing_endpoint_delete;
pub(crate) mod routing_entries;
pub(crate) mod routing_entry_cert;
pub(crate) mod routing_entry_reality;
pub(crate) mod routing_entry_selected_domain;
pub(crate) mod routing_groups;
pub(crate) mod routing_lines;
pub(crate) mod routing_local_exits;
pub(crate) mod routing_local_exits_edit;
pub(crate) mod routing_local_exits_fill;
pub(crate) mod routing_node_domains;
pub(crate) mod routing_nodes;
pub(crate) mod routing_pools;
pub(crate) mod routing_read;
pub(crate) mod routing_resources;
pub(crate) mod rows;
pub(crate) mod runtime_attach;
pub(crate) mod runtime_helpers;
pub(crate) mod security;
pub(crate) mod seed;
pub(crate) mod settings;
pub(crate) mod subscription_read;
pub(crate) mod subscription_rule_sets;
pub(crate) mod traffic_health;
pub(crate) mod traffic_health_charts;
pub(crate) mod traffic_health_items;
pub(crate) mod traffic_health_sql;
pub(crate) mod traffic_reports;
pub(crate) mod user_batch_delete;
pub(crate) mod user_delete;
pub(crate) mod user_devices;
pub(crate) mod user_traffic_logs;
pub(crate) mod users;

pub(crate) mod runtime_versions;
