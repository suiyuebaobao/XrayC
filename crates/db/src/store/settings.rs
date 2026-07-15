//! 站点设置读写和探测策略读取。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::dirty::*;
use super::existence::*;
use super::line_binding::*;
use super::probes::*;
use super::rows::*;
use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl PgStore {
    pub async fn subscription_settings_json(&self) -> Result<Value, DbError> {
        let value = self
            .site_setting_json("subscription_config", default_subscription_setting())
            .await?;
        Ok(normalize_subscription_setting(value))
    }

    pub async fn sales_landing_json(&self) -> Result<Value, DbError> {
        self.site_setting_json("sales_landing", default_sales_landing_setting())
            .await
    }

    pub async fn update_sales_landing_json(&self, value: Value) -> Result<Value, DbError> {
        let current = self.sales_landing_json().await?;
        let value = merge_json(merge_json(default_sales_landing_setting(), current), value);
        self.upsert_site_setting_json("sales_landing", value)
            .await?;
        self.sales_landing_json().await
    }

    pub async fn update_subscription_settings_json(&self, value: Value) -> Result<Value, DbError> {
        let current = self.subscription_settings_json().await?;
        let value = normalize_subscription_setting(merge_json(
            merge_json(default_subscription_setting(), current),
            value,
        ));
        self.upsert_site_setting_json("subscription_config", value)
            .await?;
        self.subscription_settings_json().await
    }

    pub async fn payment_settings_json(&self) -> Result<Value, DbError> {
        // 支付配置：库里取到的值（缺省走默认）与默认结构深合并，补齐新增字段。
        let value = self
            .site_setting_json("payment", default_payment_setting())
            .await?;
        Ok(merge_json(default_payment_setting(), value))
    }

    pub async fn public_payment_settings_json(&self) -> Result<Value, DbError> {
        // 对外可见的支付配置：四个 write-only 凭据脱敏成 <field>_set，不回显明文。
        Ok(public_payment_settings(self.payment_settings_json().await?))
    }

    pub async fn update_payment_settings_json(&self, value: Value) -> Result<Value, DbError> {
        // 读 current（含默认补齐），按 默认 → current → patch 顺序深合并，
        // 再对四个 write-only 凭据做写时保留（空值不清掉旧凭据），最后落库。
        let current = self.payment_settings_json().await?;
        let merged = merge_json(
            default_payment_setting(),
            merge_json(current.clone(), value),
        );
        let merged = crate::store::json_util::preserve_write_only_at(
            merged,
            &current,
            &PAYMENT_WRITE_ONLY_PATHS,
        );
        self.upsert_site_setting_json("payment", merged).await?;
        self.payment_settings_json().await
    }

    pub(crate) async fn subscription_blocks_unhealthy_lines(&self) -> Result<bool, DbError> {
        Ok(self
            .subscription_settings_json()
            .await?
            .get("block_unhealthy_lines")
            .and_then(Value::as_bool)
            .unwrap_or(false))
    }

    pub async fn access_operations_settings_json(&self) -> Result<Value, DbError> {
        let value = self
            .site_setting_json("access_operations", default_access_operations_setting())
            .await?;
        Ok(normalize_access_operations_setting(value))
    }

    pub async fn update_access_operations_settings_json(
        &self,
        value: Value,
    ) -> Result<Value, DbError> {
        let value = normalize_access_operations_setting(merge_json(
            default_access_operations_setting(),
            value,
        ));
        self.upsert_site_setting_json("access_operations", value)
            .await?;
        self.access_operations_settings_json().await
    }

    pub(crate) async fn access_probe_policy(&self) -> Result<AccessProbePolicy, DbError> {
        let value = self.access_operations_settings_json().await?;
        Ok(access_probe_policy_from_setting(&value))
    }

    pub(crate) async fn traffic_log_retention_policy(
        &self,
    ) -> Result<TrafficLogRetentionPolicy, DbError> {
        let value = self.access_operations_settings_json().await?;
        Ok(traffic_log_retention_policy_from_setting(&value))
    }

    pub async fn database_backup_policy(&self) -> Result<DatabaseBackupPolicy, DbError> {
        let value = self.access_operations_settings_json().await?;
        Ok(database_backup_policy_from_setting(&value))
    }

    pub async fn backup_config_json(&self) -> Result<Value, DbError> {
        // 数据库备份配置：库里取到的值（缺省走默认）经规范化返回（钳制越界、补齐新字段）。
        let value = self
            .site_setting_json("backup_config", default_backup_config())
            .await?;
        Ok(normalize_backup_config(value))
    }

    pub async fn public_backup_config_json(&self) -> Result<Value, DbError> {
        // 对外可见的备份配置：SSH 密码 / 邮件口令脱敏成 <field>_set，不回显明文。
        Ok(public_backup_config(self.backup_config_json().await?))
    }

    pub async fn update_backup_config_json(&self, value: Value) -> Result<Value, DbError> {
        // 读 current（含默认补齐+规范化），按 默认 → current → patch 深合并，
        // 再对 SSH 密码 / 邮件口令做写时保留（占位符/空不清掉旧凭据），规范化后落库。
        let current = self.backup_config_json().await?;
        let merged = merge_json(default_backup_config(), merge_json(current.clone(), value));
        let merged = crate::store::json_util::preserve_write_only_backup_config(merged, &current);
        let merged = normalize_backup_config(merged);
        self.upsert_site_setting_json("backup_config", merged)
            .await?;
        self.backup_config_json().await
    }

    pub async fn database_backup_state_json(&self) -> Result<Value, DbError> {
        self.site_setting_json("database_backup_state", json!({}))
            .await
    }

    pub async fn update_database_backup_state_json(&self, value: Value) -> Result<(), DbError> {
        self.upsert_site_setting_json("database_backup_state", value)
            .await
    }

    pub async fn try_database_backup_lock(
        &self,
    ) -> Result<Option<DatabaseBackupLockGuard>, DbError> {
        let mut tx = self.pool.begin().await?;
        let locked = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock($1)")
            .bind(DATABASE_BACKUP_LOCK_ID)
            .fetch_one(&mut *tx)
            .await?;
        if locked {
            Ok(Some(DatabaseBackupLockGuard { tx: Some(tx) }))
        } else {
            Ok(None)
        }
    }

    pub(crate) async fn site_setting_json(
        &self,
        key: &str,
        default: Value,
    ) -> Result<Value, DbError> {
        let value = sqlx::query_scalar::<_, Value>(
            "SELECT setting_value FROM site_settings WHERE setting_key = $1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(default);
        Ok(value)
    }

    pub(crate) async fn upsert_site_setting_json(
        &self,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT INTO site_settings (setting_key, setting_value, updated_at)
            VALUES ($1, $2, now())
            ON CONFLICT (setting_key) DO UPDATE SET
                setting_value = EXCLUDED.setting_value,
                updated_at = now()
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
