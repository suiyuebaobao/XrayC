//! 本文件是 XrayC Worker 进程入口。
//! Worker 负责连接 PostgreSQL，执行订单过期、订阅失效、认证临时状态
//! 清理、运行态快照保留和出口组合探测排队策略，避免 API 进程承担
//! 后台扫描职责。

mod backups;

use std::time::Duration;

use backups::DatabaseBackupRunner;
use xrayc_db::PgStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "xrayc_worker=info".into()))
        .init();

    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL 未配置"))?;
    let migrations_path = std::env::var("MIGRATIONS_DIR")
        .or_else(|_| std::env::var("MIGRATIONS_PATH"))
        .unwrap_or_else(|_| "migrations".to_string());
    let interval = std::env::var("WORKER_TICK_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60);
    let runtime_retention_days = std::env::var("WORKER_RUNTIME_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(14);
    let backup_runner = DatabaseBackupRunner::from_env(database_url.clone());
    let store = PgStore::connect(&database_url).await?;
    store.migrate(&migrations_path).await?;

    let stale_deploy_threshold = stale_deployment_threshold_seconds();
    tracing::info!(
        interval,
        runtime_retention_days,
        stale_deploy_threshold,
        "starting xrayc worker"
    );
    loop {
        match store.run_worker_maintenance(runtime_retention_days).await {
            Ok(result) => tracing::info!(?result, "worker maintenance finished"),
            Err(error) => tracing::error!(?error, "worker maintenance failed"),
        }
        // 卡死自愈:把超过阈值未推进的非终态部署任务标记为 failed,
        // 避免失败/回滚的一键安装永远停在 waiting_for_server。
        match store
            .mark_stale_deployment_tasks_failed(stale_deploy_threshold)
            .await
        {
            Ok(failed) => {
                if failed > 0 {
                    tracing::info!(failed, "stale deployment tasks marked failed");
                }
            }
            Err(error) => tracing::error!(?error, "stale deployment task scan failed"),
        }
        // 一轮编排四模式(全量/异地/WAL/邮件),各模式错误已在内部脱敏隔离,
        // 只有底层 DB/锁故障才上抛到这里记 error。
        match backup_runner.run_all_if_due(&store).await {
            Ok(report) => tracing::info!(?report, "database backup round finished"),
            Err(error) => tracing::error!(?error, "database backup round failed"),
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
}

/// 部署任务卡死阈值(秒):非终态任务超过此时长未推进会被标记 failed。
/// 默认 15 分钟(900 秒),可用 WORKER_STALE_DEPLOY_TASK_SECONDS 覆盖。
/// 任何非法或过小取值都回落到默认值,避免误把刚创建的任务扫成失败。
fn stale_deployment_threshold_seconds() -> i64 {
    parse_stale_deployment_threshold(std::env::var("WORKER_STALE_DEPLOY_TASK_SECONDS").ok())
}

/// 纯解析:把已读出的原始环境值(Option<String>)归一为阈值秒数。抽成纯函数便于无 env 单测
/// ——原先三个测试共享同一进程级环境变量、cargo 并行跑会相互串扰导致 flaky 失败。
/// 非法或过小(<60s)取值回落默认 900,避免误把刚创建的任务扫成失败。
fn parse_stale_deployment_threshold(raw: Option<String>) -> i64 {
    const DEFAULT_STALE_DEPLOY_TASK_SECONDS: i64 = 900;
    raw.and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 60)
        .unwrap_or(DEFAULT_STALE_DEPLOY_TASK_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测纯解析函数 parse_stale_deployment_threshold(不碰进程级 env,避免并行测试串扰致 flaky)。
    #[test]
    fn stale_threshold_defaults_to_fifteen_minutes() {
        assert_eq!(parse_stale_deployment_threshold(None), 900);
        assert_eq!(
            parse_stale_deployment_threshold(Some("not-a-number".into())),
            900
        );
    }

    #[test]
    fn stale_threshold_honors_valid_override() {
        assert_eq!(parse_stale_deployment_threshold(Some("1800".into())), 1800);
    }

    #[test]
    fn stale_threshold_rejects_too_small_override() {
        // 过小取值(<60s)会误杀刚创建任务,必须回落到默认 900。
        assert_eq!(parse_stale_deployment_threshold(Some("5".into())), 900);
        assert_eq!(parse_stale_deployment_threshold(Some("59".into())), 900);
    }
}
