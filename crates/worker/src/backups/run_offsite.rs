//! 各模式落盘成功后的「异地并入」处理(从 run.rs 拆出,收敛 rsync 推送 + 删本机 + 结果并入 state)。
//! 全量走 apply_full_offsite(按 full.destination 推 dump、offsite 且成功后删本机 dump),
//! WAL 走 apply_wal_offsite(按 wal.destination 推归档+base 仓库、offsite 且成功后清本机 base/归档)。
//! 两者都:destination=local 直接返回;推送失败只脱敏记 offsite_error、本机一律保留(绝不无备份);
//! 结果经 merge_mode_state **增量合并**进对应方式 state(offsite_synced/offsite_last_sync_at 等),
//! 绝不翻转全量/base 成功、绝不上抛、绝不 panic(错误隔离红线)。删除动作一律在 rsync 成功之后。
//! 方法标 `pub(super)`:定义在 backups::run_offsite,供兄弟模块 backups::run 的主流程调用。
//! 敏感值(host/密码/rsync 原始错误)在写入 state 前必过 secrets 脱敏。
//! 本头部满足前十行中文说明约束。

use chrono::Utc;
use serde_json::{json, Value};
use xrayc_db::PgStore;

use super::{merge_mode_state, offsite, secrets, wal, DatabaseBackupRunner};

impl DatabaseBackupRunner {
    /// 全量成功后的异地处理(按 full.destination):
    /// - `local`:什么都不做(仅本机);
    /// - `both`:推异地、本机保留;
    /// - `offsite`:推异地,**推成功后删本机该 dump 文件**;推失败保留本机(绝不无备份)。
    ///
    /// 异地结果并入 full state,失败只脱敏记 error、不翻转全量成功、不上抛(错误隔离)。
    pub(super) async fn apply_full_offsite(
        &self,
        store: &PgStore,
        config: &Value,
        state: &mut Value,
        file_name: &str,
        secrets: &[String],
    ) {
        let destination = config
            .get("full")
            .and_then(|full| full.get("destination"))
            .and_then(Value::as_str)
            .unwrap_or("local");
        let plan = offsite::destination_plan(destination);
        if !plan.should_push() {
            return;
        }
        let offsite = config.get("offsite").cloned().unwrap_or_else(|| json!({}));
        let sources = [self.backup_dir.as_path()];
        let (synced, message) = match offsite::sync_offsite(&offsite, &self.key_dir, &sources).await
        {
            Ok(report) => (
                report.synced,
                secrets::sanitize_message(&report.message, secrets),
            ),
            Err(error) => (
                false,
                secrets::sanitize_message(&error.to_string(), secrets),
            ),
        };

        let mut patch = json!({ "offsite_synced": synced, "offsite_message": message.clone() });
        if synced {
            patch["offsite_last_sync_at"] = json!(Utc::now().to_rfc3339());
        } else {
            // 推失败:记脱敏 error,本机 dump 一律保留(绝不无备份)。
            patch["offsite_error"] = json!(message);
        }
        // 仅异地(offsite)且已确认推送成功 → 删本机该 dump(红线:删除必在 rsync 成功之后)。
        if offsite::should_delete_local_after_sync(plan, synced) {
            let backup_path = self.backup_dir.join(file_name);
            match std::fs::remove_file(&backup_path) {
                Ok(()) => {
                    patch["local_removed"] = json!(true);
                }
                Err(error) => {
                    patch["local_removed"] = json!(false);
                    patch["local_remove_error"] =
                        json!(secrets::sanitize_message(&error.to_string(), secrets));
                }
            }
        }
        merge_mode_state(state, "full", patch);
        if let Err(error) = self.persist_state(store, state).await {
            tracing::warn!(?error, "full offsite state update failed");
        }
    }

    /// WAL base 成功后的异地处理(按 wal.destination):
    /// - `local`:不推异地;
    /// - `both`:推异地(归档卷 + base 仓库卷),本机保留;
    /// - `offsite`:推异地,**推成功后清本机 base/归档**(best-effort);推失败保留本机(绝不无备份)。
    ///
    /// 异地结果并入 wal state(offsite_synced/offsite_last_sync_at/offsite_error),不翻转 base 成功、不上抛。
    pub(super) async fn apply_wal_offsite(
        &self,
        store: &PgStore,
        config: &Value,
        state: &mut Value,
        secrets: &[String],
    ) {
        let destination = config
            .get("wal")
            .and_then(|wal| wal.get("destination"))
            .and_then(Value::as_str)
            .unwrap_or("local");
        let plan = offsite::destination_plan(destination);
        if !plan.should_push() {
            return;
        }
        let offsite = config.get("offsite").cloned().unwrap_or_else(|| json!({}));
        let (synced, message) = match wal::sync_wal_offsite(&offsite, &self.key_dir).await {
            Ok(report) => (
                report.synced,
                secrets::sanitize_message(&report.message, secrets),
            ),
            Err(error) => (
                false,
                secrets::sanitize_message(&error.to_string(), secrets),
            ),
        };

        let mut patch = json!({ "offsite_synced": synced, "offsite_message": message.clone() });
        if synced {
            patch["offsite_last_sync_at"] = json!(Utc::now().to_rfc3339());
        } else {
            patch["offsite_error"] = json!(message);
        }
        // 仅异地(offsite)且推送成功 → 清本机 base/归档(红线:删除必在 rsync 成功之后)。
        if offsite::should_delete_local_after_sync(plan, synced) {
            let (bases, archives) =
                wal::clear_local_wal(&self.wal_repo_dir, &self.wal_archive_dir).await;
            patch["local_cleared_bases"] = json!(bases);
            patch["local_cleared_archives"] = json!(archives);
        }
        merge_mode_state(state, "wal", patch);
        if let Err(error) = self.persist_state(store, state).await {
            tracing::warn!(?error, "wal offsite state update failed");
        }
    }
}
