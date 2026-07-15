//! 一键安装重装熔断统计。
//! 从 deployment_tasks 拆出,保持单文件不超 550 行硬上限(CLAUDE.md §8)。
//! 仅统计同一 SSH 目标最近窗口内的强制重装一键安装任务数,供 API 侧设上限熔断。
//! 禁止"见 failed 就无条件 forced-reinstall"导致把快装好的 agent 清掉重来的自伤式重装风暴。
//! 只读统计,不修改任何任务记录、不持有凭据、不执行远程操作。
//! SQL 通过 PgStore 连接池执行,只读 safe_metadata 脱敏字段,不触碰敏感列。
//! 失败重试也计入,以 created_at 为准(每次一键安装都新建任务)。
//! 后端一键安装一律 force_reinstall=true,故 mode=one_click 即等价强制重装。
//! 文件头部中文注释满足仓库规则。
//! 不在日志中输出服务器地址或其它敏感信息。

use crate::*;

impl PgStore {
    /// 数同一 SSH 目标最近窗口内的强制重装一键安装任务数,供调用方设上限熔断。
    pub async fn count_recent_forced_reinstalls_for_ssh_host(
        &self,
        ssh_host: &str,
        window_seconds: i64,
    ) -> Result<i64, DbError> {
        let ssh_host = ssh_host.trim();
        if ssh_host.is_empty() {
            return Ok(0);
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM deployment_tasks \
             WHERE kind = 'agent_install' AND safe_metadata ->> 'mode' = 'one_click' \
             AND safe_metadata ->> 'ssh_host' = $1 \
             AND created_at >= now() - ($2::BIGINT * interval '1 second')",
        )
        .bind(ssh_host)
        .bind(window_seconds.max(1))
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }
}
