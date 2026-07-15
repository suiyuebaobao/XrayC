//! 本模块存放 worker 后台维护相关的公开结果类型。
//! 从 types 模块拆出，专门承载 Worker 周期性清理任务的统计返回。
//! 只包含轻量结构体定义，不含 SQL 查询与持久化流程。
//! 各字段均为无符号计数，描述一次维护清理的各类命中数量。
//! 结构体保持 pub 可见性与原 derive，round-trip 序列化行为不变。
//! 通过 crate root 的 `pub use` 重新导出，保持历史 public API 不变。
//! `xrayc_db::WorkerMaintenanceResult` 与 `crate::WorkerMaintenanceResult` 导入路径均不变。
//! store::maintenance 经 `use crate::*` 引用本类型，拆分后仍能解析。
//! 新增 worker 维护相关的 DTO 应优先放入本模块，避免 types 再次膨胀。
//! 本文件不引入额外外部依赖，serde 派生走全路径写法。

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct WorkerMaintenanceResult {
    pub expired_subscriptions: u64,
    pub expired_orders: u64,
    pub expired_refresh_tokens: u64,
    pub expired_auth_challenges: u64,
    pub expired_login_guards: u64,
    pub timed_out_exit_probe_tasks: u64,
    pub queued_exit_probe_tasks: u64,
    pub pruned_metric_snapshots: u64,
    pub pruned_traffic_snapshots: u64,
    pub pruned_user_sessions: u64,
    pub pruned_user_session_events: u64,
    pub pruned_line_probes: u64,
    pub pruned_exit_probes: u64,
    pub pruned_usage_ledgers: u64,
}
