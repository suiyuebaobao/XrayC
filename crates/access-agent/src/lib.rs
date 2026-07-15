//! 本文件是 access-agent crate 的公开入口。
//! 这里统一导出 HTTP 客户端、环境配置和运行时循环，供二进制入口
//! 与集成测试复用。

pub mod client;
#[cfg(test)]
mod client_tests;
pub mod config;
pub mod runtime;
pub mod stats;

pub use client::{
    AccessExitProbe, AccessLineMetric, AccessLineProbe, AccessUserSession, AgentClient,
    ConfigResultRequest, ConfigResultResponse, HeartbeatRequest, HeartbeatResponse,
    MetricsReportRequest, MetricsReportResponse, NodeMetricsReport, ProbesReportRequest,
    ProbesReportResponse, SessionsReportRequest, SessionsReportResponse, TrafficReportRequest,
    TrafficReportResponse, TrafficSnapshot,
};
pub use config::AgentSettings;
