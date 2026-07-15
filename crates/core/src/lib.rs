//! 本文件导出 XrayC 核心领域和服务逻辑。
//! 本 crate 维护中转入口订阅模型、流量快照计费，以及早期开发阶段
//! 供 Axum API 使用的内存存储。

pub mod billing;
pub mod credentials;
pub mod host_metrics;
pub mod model;
pub mod runtime_ports;
pub mod store;
pub mod subscription;

pub use billing::{TrafficReport, TrafficReportResult, TrafficService};
pub use credentials::binding_credential;
pub use model::*;
pub use runtime_ports::{
    effective_user_rate_limit_bps, effective_user_rate_limit_down_bps,
    effective_user_rate_limit_up_bps, rate_limit_mark_for_user, rate_limit_marks_for_node,
    runtime_listen_port_for_user, user_needs_rate_limit,
};
pub use store::{MemoryStore, StoreData};
pub use subscription::{
    generate_clash_yaml, generate_clash_yaml_with_options, SubscriptionError, SubscriptionOptions,
};
