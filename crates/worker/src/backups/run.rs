//! 各备份模式的执行与状态回写(编排层 mod.rs 的薄接线;IO 都收敛在此)。
//! 每个 `run_*` 方法自行捕获错误、把错误摘要过 `secrets` 脱敏后写回对应模式的 state 子对象,
//! 并返回 `ModeStatus` 供日志——**绝不上抛、绝不 panic、绝不中断其它模式**(红线)。
//! 全量走 `dump`(pg_dump),异地走 `offsite`(rsync 免密),WAL 走 `wal`(pg_basebackup + 归档
//! 清理 + 推异地),邮件走 `email`(附件加密 + 共享 SMTP 发送,失败转发失败通知)。
//! SMTP 从 `auth_security.email_verification` 取(明文口令只在内存用、绝不落状态/日志)。
//! 状态回写只更新本模式的顶层键(经 mod.rs 的 set_mode_state),不影响其它模式与 run_now。
//! 本头部满足前十行中文说明约束。

use anyhow::{anyhow, Context};
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use xrayc_db::PgStore;

use super::dump::{self, PgDumpConnection};
use super::{
    append_mode_history, email, history_entry, secrets, set_mode_state, wal, DatabaseBackupRunner,
    ModeStatus,
};

impl DatabaseBackupRunner {
    /// 全量 pg_dump:落地到 backup_dir、按 full.retention_days 清理本地旧备份,状态写回 state.full。
    /// 读 full.exclude_traffic_logs 决定是否排 7 张观测大表。失败时记失败态(含退避 next_attempt_at)
    /// 并返回 Err(已脱敏),由编排层归入「全量失败但不中断其它模式」。成功返回本次 dump 文件名。
    pub(super) async fn run_full_backup(
        &self,
        store: &PgStore,
        config: &Value,
        state: &mut Value,
    ) -> anyhow::Result<String> {
        let full = config.get("full").cloned().unwrap_or_else(|| json!({}));
        let retention_days = full
            .get("retention_days")
            .and_then(Value::as_i64)
            .unwrap_or(30);
        let exclude_traffic_logs = full
            .get("exclude_traffic_logs")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        std::fs::create_dir_all(&self.backup_dir)
            .with_context(|| format!("create backup dir {:?}", self.backup_dir))?;
        let started_at = Utc::now();
        set_mode_state(
            state,
            "full",
            json!({
                "status": "running",
                "last_started_at": started_at.to_rfc3339(),
                "backup_dir": self.backup_dir.to_string_lossy(),
            }),
        );
        self.persist_state(store, state).await?;

        let file_name = dump::backup_file_name(started_at);
        let backup_path = self.backup_dir.join(&file_name);
        let secrets = secrets::backup_secrets(&self.database_url);
        match self.create_dump(&backup_path, exclude_traffic_logs).await {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let _ = std::fs::remove_file(&backup_path);
                let error_summary = secrets::sanitize_command_output(&output.stderr, &secrets);
                self.record_full_failure(store, state, started_at, &error_summary)
                    .await?;
                return Err(anyhow!("pg_dump failed: {error_summary}"));
            }
            Err(error) => {
                let _ = std::fs::remove_file(&backup_path);
                let error_summary = secrets::sanitize_text(error.to_string(), &secrets);
                self.record_full_failure(store, state, started_at, &error_summary)
                    .await?;
                return Err(anyhow!("pg_dump failed: {error_summary}"));
            }
        }

        // 全量成功后先取 dump 文件字节:destination=offsite 时 apply_full_offsite 会删本机 dump,
        // 大小必须在删之前取,否则取到已删文件的 0(历史大小失真红线)。
        let dump_size_bytes = std::fs::metadata(&backup_path)
            .map(|meta| meta.len())
            .unwrap_or(0);

        let completed_at = Utc::now();
        set_mode_state(
            state,
            "full",
            json!({
                "status": "success",
                "last_started_at": started_at.to_rfc3339(),
                "last_success_at": completed_at.to_rfc3339(),
                "last_file_name": file_name.clone(),
                "backup_dir": self.backup_dir.to_string_lossy(),
                "prune_status": "pending",
            }),
        );
        self.persist_state(store, state).await?;

        // 本地保留清理:失败只降级记状态,不翻转全量成功(备份产物已在盘)。
        match dump::prune_old_backup_files(&self.backup_dir, retention_days, Utc::now()).await {
            Ok(pruned_files) => set_mode_state(
                state,
                "full",
                json!({
                    "status": "success",
                    "last_started_at": started_at.to_rfc3339(),
                    "last_success_at": completed_at.to_rfc3339(),
                    "last_file_name": file_name.clone(),
                    "backup_dir": self.backup_dir.to_string_lossy(),
                    "last_pruned_at": Utc::now().to_rfc3339(),
                    "pruned_files": pruned_files,
                    "prune_status": "success",
                }),
            ),
            Err(error) => {
                let error_summary = secrets::sanitize_text(error.to_string(), &secrets);
                tracing::warn!(summary = %error_summary, "database backup prune failed");
                set_mode_state(
                    state,
                    "full",
                    json!({
                        "status": "success",
                        "last_started_at": started_at.to_rfc3339(),
                        "last_success_at": completed_at.to_rfc3339(),
                        "last_file_name": file_name.clone(),
                        "backup_dir": self.backup_dir.to_string_lossy(),
                        "last_pruned_at": Utc::now().to_rfc3339(),
                        "prune_status": "failed",
                        "prune_error_summary": error_summary,
                    }),
                );
            }
        }
        if let Err(error) = self.persist_state(store, state).await {
            tracing::warn!(?error, "database backup prune state update failed");
        }

        // 全量落盘成功后按 full.destination 处理异地(both/offsite):推异地、按需删本机,
        // 结果并入 full state(offsite_synced/offsite_last_sync_at/offsite_error);绝不翻转全量成功、绝不上抛。
        self.apply_full_offsite(store, config, state, &file_name, &secrets)
            .await;

        // 追加一条全量历史(独立顶层 history.full,set_mode_state/merge 都不碰它)。
        // size 用删前取的 dump_size_bytes;offsite_synced 取本轮并入 full state 的结果
        // (destination=local 无该字段 → None → null,符合「未推异地」语义)。
        let offsite_synced = state
            .get("full")
            .and_then(|full| full.get("offsite_synced"))
            .and_then(Value::as_bool);
        append_mode_history(
            state,
            "full",
            history_entry(
                Utc::now().to_rfc3339(),
                true,
                dump_size_bytes,
                Some(file_name.clone()),
                offsite_synced,
                None,
            ),
        );
        if let Err(error) = self.persist_state(store, state).await {
            tracing::warn!(?error, "full history state update failed");
        }
        Ok(file_name)
    }

    /// 全量失败态写回(含失败退避 next_attempt_at,供 schedule 判 due 时退避重试)。
    async fn record_full_failure(
        &self,
        store: &PgStore,
        state: &mut Value,
        started_at: chrono::DateTime<Utc>,
        error_summary: &str,
    ) -> anyhow::Result<()> {
        let failed_at = Utc::now();
        let next_attempt_at = failed_at + Duration::seconds(self.retry_delay.as_secs() as i64);
        set_mode_state(
            state,
            "full",
            json!({
                "status": "failed",
                "last_started_at": started_at.to_rfc3339(),
                "last_failed_at": failed_at.to_rfc3339(),
                "next_attempt_at": next_attempt_at.to_rfc3339(),
                "backup_dir": self.backup_dir.to_string_lossy(),
                "error_summary": error_summary,
            }),
        );
        // 追加一条全量失败历史(两条失败分支都经此;error_summary 已脱敏)。
        append_mode_history(
            state,
            "full",
            history_entry(
                failed_at.to_rfc3339(),
                false,
                0,
                None,
                None,
                Some(error_summary.to_string()),
            ),
        );
        self.persist_state(store, state).await
    }

    /// WAL base 备份:pg_basebackup → 归档清理 → 一并推异地,状态写回 state.wal。
    /// base 成功后归档清理 / 推异地都只降级记录、不翻转 base 成功;base 失败记失败态。全程不上抛。
    pub(super) async fn run_wal(
        &self,
        store: &PgStore,
        config: &Value,
        state: &mut Value,
        secrets: &[String],
    ) -> ModeStatus {
        let wal = config.get("wal").cloned().unwrap_or_else(|| json!({}));
        let retention_full = wal
            .get("retention_full")
            .and_then(Value::as_i64)
            .unwrap_or(4);
        let started_at = Utc::now();

        // 连接要素解析失败(DATABASE_URL 异常)也只脱敏记失败,不上抛。
        let conn = match PgDumpConnection::parse(&self.database_url) {
            Ok(conn) => conn,
            Err(error) => {
                let summary = secrets::sanitize_message(&error.to_string(), secrets);
                let failed_at = Utc::now();
                set_mode_state(
                    state,
                    "wal",
                    json!({
                        "last_started_at": started_at.to_rfc3339(),
                        "last_failed_at": failed_at.to_rfc3339(),
                        "error_summary": summary,
                    }),
                );
                append_mode_history(
                    state,
                    "wal",
                    history_entry(failed_at.to_rfc3339(), false, 0, None, None, Some(summary)),
                );
                if let Err(error) = self.persist_state(store, state).await {
                    tracing::warn!(?error, "wal state update failed");
                }
                return ModeStatus::Failed;
            }
        };

        let status = match wal::run_base_backup(&conn, &self.wal_repo_dir, retention_full).await {
            Ok(report) => {
                // 删前先取本次 base 目录总字节:destination=offsite 时 apply_wal_offsite 会清本机 base,
                // 大小须在清理之前递归求和,否则取到已清目录的 0(历史大小失真红线)。
                let base_dir_name = report.base_dir_name.clone();
                let base_size_bytes = wal::dir_total_bytes(&self.wal_repo_dir.join(&base_dir_name));
                // 归档清理:失败只降级(保守不删已算安全),记 wal_last_archive_ok。
                let prune = wal::prune_wal_archive(&self.wal_archive_dir, &self.wal_repo_dir).await;
                let archive_ok = prune.is_ok();
                let archive_pruned = *prune.as_ref().unwrap_or(&0);
                let archive_error = prune
                    .err()
                    .map(|error| secrets::sanitize_message(&error.to_string(), secrets));
                set_mode_state(
                    state,
                    "wal",
                    json!({
                        "last_started_at": started_at.to_rfc3339(),
                        "last_success_at": Utc::now().to_rfc3339(),
                        "base_dir_name": report.base_dir_name,
                        "pruned_bases": report.pruned_bases,
                        "wal_last_archive_ok": archive_ok,
                        "archive_pruned": archive_pruned,
                        "archive_error_summary": archive_error,
                    }),
                );
                // 按 wal.destination 处理异地(both/offsite):推异地、offsite 且成功则清本机 base/归档;
                // 结果并入 wal state(offsite_synced/offsite_last_sync_at/offsite_error),不翻转 base 成功。
                self.apply_wal_offsite(store, config, state, secrets).await;
                // 追加一条 WAL 成功历史:size 用清理前取的 base 目录总字节;offsite_synced 取本轮
                // 并入 wal state 的结果(destination=local 无该字段 → None → null)。
                let offsite_synced = state
                    .get("wal")
                    .and_then(|wal| wal.get("offsite_synced"))
                    .and_then(Value::as_bool);
                append_mode_history(
                    state,
                    "wal",
                    history_entry(
                        Utc::now().to_rfc3339(),
                        true,
                        base_size_bytes,
                        Some(base_dir_name),
                        offsite_synced,
                        None,
                    ),
                );
                ModeStatus::Ran
            }
            Err(error) => {
                let summary = secrets::sanitize_message(&error.to_string(), secrets);
                let failed_at = Utc::now();
                set_mode_state(
                    state,
                    "wal",
                    json!({
                        "last_started_at": started_at.to_rfc3339(),
                        "last_failed_at": failed_at.to_rfc3339(),
                        "error_summary": summary,
                    }),
                );
                append_mode_history(
                    state,
                    "wal",
                    history_entry(failed_at.to_rfc3339(), false, 0, None, None, Some(summary)),
                );
                ModeStatus::Failed
            }
        };
        if let Err(error) = self.persist_state(store, state).await {
            tracing::warn!(?error, "wal state update failed");
        }
        status
    }

    /// 邮件发文件:取 SMTP(auth_security.email_verification)→ 取最新 dump → 加密附件发送。
    /// SMTP 取不到 / 无可发 dump → 跳过并记原因(不算失败);MissingPassphrase / 发送失败 →
    /// 脱敏 detail 后转发「失败通知」并记失败态。全程不上抛。
    pub(super) async fn run_email(
        &self,
        store: &PgStore,
        config: &Value,
        state: &mut Value,
        secrets: &[String],
    ) -> ModeStatus {
        let email = config.get("email").cloned().unwrap_or_else(|| json!({}));
        let started_at = Utc::now();

        // 取 SMTP:auth_security → email_verification → smtp_config_from_email_verification。
        let smtp = match store.admin_auth_security_json().await {
            Ok(security) => security
                .get("email_verification")
                .and_then(xrayc_backup::smtp_config_from_email_verification),
            Err(error) => {
                let summary = secrets::sanitize_message(&error.to_string(), secrets);
                return self
                    .record_email_skip(
                        store,
                        state,
                        started_at,
                        &format!("读取 SMTP 配置失败: {summary}"),
                    )
                    .await;
            }
        };
        let Some(smtp) = smtp else {
            return self
                .record_email_skip(store, state, started_at, "SMTP 配置不完整,跳过邮件发送")
                .await;
        };

        // 取最新全量 dump 作附件源(run_now==email 时可取上一轮已存在的产物)。
        let Some(dump_path) = dump::latest_dump_file(&self.backup_dir) else {
            return self
                .record_email_skip(store, state, started_at, "无可发送的备份文件,跳过邮件")
                .await;
        };

        // 精简版 dump 目前不单独产出(全量按配置产一份),故 smaller 传 None;
        // 附件超上限时由 send_backup_email 内部降级为「仅通知」。
        let status = match email::send_backup_email(&email, &smtp, &dump_path, None).await {
            Ok(report) => {
                // 先取历史所需字段:attachment_name 随后被 email_state 移走,故先克隆。
                let skipped = report.skipped;
                let attachment_bytes = report.attachment_bytes;
                let attachment_name = report.attachment_name.clone();
                let mut email_state = json!({
                    "last_started_at": started_at.to_rfc3339(),
                    "skipped": report.skipped,
                    "attached": report.attached,
                    "encrypted": report.encrypted,
                    "attachment_name": report.attachment_name,
                    "attachment_bytes": report.attachment_bytes,
                    "split_parts": report.split_parts,
                });
                // 真发送成功才写 last_success_at + email_last_sent_at(状态派生为 success);
                // 跳过(如无收件人)不算成功,不写这两项(状态派生不会落到 success)。
                if !skipped {
                    let sent_at = Utc::now().to_rfc3339();
                    email_state["last_success_at"] = json!(sent_at);
                    email_state["email_last_sent_at"] = json!(sent_at);
                }
                set_mode_state(state, "email", email_state);
                // 追加一条邮件历史:真发送成功 → ok=true + 附件字节/名(size 取 EmailSendReport);
                // 无收件人跳过 → ok=false + 原因(未发出、不计成功)。邮件不推异地,offsite_synced 恒 None。
                if skipped {
                    append_mode_history(
                        state,
                        "email",
                        history_entry(
                            Utc::now().to_rfc3339(),
                            false,
                            0,
                            None,
                            None,
                            Some("无收件人,跳过发送".to_string()),
                        ),
                    );
                    ModeStatus::Skipped
                } else {
                    append_mode_history(
                        state,
                        "email",
                        history_entry(
                            Utc::now().to_rfc3339(),
                            true,
                            attachment_bytes,
                            attachment_name,
                            None,
                            None,
                        ),
                    );
                    ModeStatus::Ran
                }
            }
            Err(error) => {
                // 捕获 MissingPassphrase / 发送失败 → 脱敏后转「失败通知」(通知本身失败也不影响)。
                let detail = secrets::sanitize_message(&error.to_string(), secrets);
                if let Err(notify_error) =
                    email::send_notification(&email, &smtp, false, &detail).await
                {
                    let notify_summary =
                        secrets::sanitize_message(&notify_error.to_string(), secrets);
                    tracing::warn!(summary = %notify_summary, "backup failure notification send failed");
                }
                let failed_at = Utc::now();
                set_mode_state(
                    state,
                    "email",
                    json!({
                        "last_started_at": started_at.to_rfc3339(),
                        "last_failed_at": failed_at.to_rfc3339(),
                        "error_summary": detail,
                    }),
                );
                append_mode_history(
                    state,
                    "email",
                    history_entry(failed_at.to_rfc3339(), false, 0, None, None, Some(detail)),
                );
                ModeStatus::Failed
            }
        };
        if let Err(error) = self.persist_state(store, state).await {
            tracing::warn!(?error, "email state update failed");
        }
        status
    }

    /// 邮件主动跳过的统一记录:落 state.email(skipped=true + 原因),返回 Skipped。
    async fn record_email_skip(
        &self,
        store: &PgStore,
        state: &mut Value,
        started_at: chrono::DateTime<Utc>,
        reason: &str,
    ) -> ModeStatus {
        set_mode_state(
            state,
            "email",
            json!({
                "last_started_at": started_at.to_rfc3339(),
                "skipped": true,
                "error_summary": reason,
            }),
        );
        // 主动跳过(SMTP 缺失/无可发 dump 等)也追加一条历史:ok=false + 脱敏原因,便于详情页
        // 解释「这轮为何没发邮件」。reason 是固定文案或已脱敏摘要,不含敏感值。
        append_mode_history(
            state,
            "email",
            history_entry(
                Utc::now().to_rfc3339(),
                false,
                0,
                None,
                None,
                Some(reason.to_string()),
            ),
        );
        if let Err(error) = self.persist_state(store, state).await {
            tracing::warn!(?error, "email skip state update failed");
        }
        ModeStatus::Skipped
    }
}
