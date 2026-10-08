//! 数据库单元测试聚合模块。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use crate::store::dirty::*;
use crate::store::existence::*;
use crate::store::line_binding::*;
use crate::store::probes::*;
use crate::store::routing_entries::access_entry_network_mode;
use crate::store::runtime_helpers::*;
use crate::store::security::*;
use crate::xray_protocol::endpoint_config_is_empty;

include!("part01.rs");
include!("part02.rs");
include!("part03.rs");
include!("part04.rs");
include!("part05.rs");
include!("part06.rs");
include!("part07.rs");
include!("part08.rs");
include!("part09.rs");
include!("part10.rs");
include!("part11.rs");
include!("part12.rs");
include!("part13.rs");
include!("part14.rs");
include!("part15.rs");
include!("part16.rs");
include!("part17.rs");
include!("part18.rs");
include!("part19.rs");
include!("part20.rs");
include!("part21.rs");
include!("part23.rs");
include!("part24.rs");
include!("part25.rs");
include!("part26.rs");
include!("part27.rs");
include!("part28.rs");
include!("part29.rs");
include!("part30.rs");
include!("part31.rs");
include!("part32.rs");
include!("part33.rs");
include!("part34.rs");
include!("part35.rs");
include!("part36.rs");
include!("part37.rs");
include!("part38.rs");
include!("part39.rs");
include!("part40.rs");
include!("part41.rs");
include!("part42.rs");
include!("part43.rs");
include!("part44.rs");
include!("part45.rs");
include!("part46.rs");
include!("part47.rs");
include!("part48.rs");
include!("part49.rs");
include!("part50.rs");
include!("part51.rs");
include!("part52.rs");
include!("part53.rs");
include!("part54.rs");
include!("part55.rs");
include!("part56.rs");
include!("part57.rs");
include!("part58.rs");
include!("part59.rs");
include!("part60.rs");
include!("part61.rs");
include!("part62.rs");
include!("part63.rs");
include!("part64.rs");
include!("part65.rs");
include!("part66.rs");
include!("part67.rs");
include!("part68.rs");
include!("part69.rs");
include!("part70.rs");
include!("part71.rs");
include!("part72.rs");
include!("unlimited_traffic.rs");
include!("routing_history.rs");
include!("maintenance_isolation.rs");
include!("heartbeat_cache.rs");
include!("node_traffic_history.rs");
include!("runtime_versions.rs");
include!("deployment_progress.rs");
