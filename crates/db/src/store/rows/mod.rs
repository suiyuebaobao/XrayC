//! 数据库查询行模型聚合模块。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
mod deployment_tasks;
pub(crate) use deployment_tasks::*;
mod topology;
pub(crate) use topology::*;
mod routing;
pub(crate) use routing::*;
mod auth;
pub(crate) use auth::*;
mod assignments;
pub(crate) use assignments::*;
mod operations;
pub(crate) use operations::*;
mod users_orders;
pub(crate) use users_orders::*;
