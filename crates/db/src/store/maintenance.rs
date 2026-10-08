//! Worker 维护任务编排。每个任务独立事务及数据库互斥锁，单项失败不回滚其他工作。
//! 兼容入口仍执行全部任务；Worker 正式调度使用各任务独立循环，避免备份阻塞到期处理。
use super::maintenance_jobs::*;
use super::maintenance_usage_rollups::roll_up_and_prune_usage_ledgers_in_tx;
use crate::*;

impl PgStore {
    pub async fn run_worker_maintenance(
        &self,
        runtime_retention_days: i64,
    ) -> Result<WorkerMaintenanceResult, DbError> {
        let mut result = WorkerMaintenanceResult::default();
        let mut first_error = None;
        for job in WorkerMaintenanceJob::ALL {
            match self.run_worker_job(job, runtime_retention_days).await {
                Ok(part) => result.accumulate(part),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(result)
    }

    pub async fn run_worker_job(
        &self,
        job: WorkerMaintenanceJob,
        runtime_retention_days: i64,
    ) -> Result<WorkerMaintenanceResult, DbError> {
        let mut tx = self.pool.begin().await?;
        let acquired =
            sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock(20261005, $1)")
                .bind(job as i32)
                .fetch_one(&mut *tx)
                .await?;
        if !acquired {
            return Ok(WorkerMaintenanceResult::default());
        }
        let mut users_to_resync = std::collections::HashSet::new();
        let result = match job {
            WorkerMaintenanceJob::Lifecycle => maintain_lifecycle(&mut tx).await?,
            WorkerMaintenanceJob::Authentication => maintain_auth(&mut tx).await?,
            WorkerMaintenanceJob::RuntimeRetention => {
                maintain_runtime(&mut tx, runtime_retention_days.clamp(1, 3650)).await?
            }
            WorkerMaintenanceJob::UsageRollups => {
                let policy = self.traffic_log_retention_policy().await?;
                let pruned_usage_ledgers = if policy.prune_enabled {
                    roll_up_and_prune_usage_ledgers_in_tx(&mut tx, policy).await?
                } else {
                    0
                };
                WorkerMaintenanceResult {
                    pruned_usage_ledgers,
                    ..Default::default()
                }
            }
            WorkerMaintenanceJob::Probes => {
                let (timed_out_exit_probe_tasks, users) = self
                    .timeout_expired_access_exit_probe_tasks_in_tx(&mut tx)
                    .await?;
                users_to_resync = users;
                let queued_exit_probe_tasks = self
                    .queue_due_access_exit_probe_tasks_in_tx(&mut tx)
                    .await?;
                WorkerMaintenanceResult {
                    timed_out_exit_probe_tasks,
                    queued_exit_probe_tasks,
                    ..Default::default()
                }
            }
        };
        tx.commit().await?;
        for user_id in users_to_resync {
            self.sync_user_exit_assignments(user_id).await?;
        }
        Ok(result)
    }
}
