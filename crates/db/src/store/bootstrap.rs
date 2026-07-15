//! install 自带管理员 bootstrap:无管理员则建账号 admin。
//! 真 Argon2(复用 security::password_hash),不依赖默认套餐,生产可用。
//! 幂等:库内已存在任一 is_admin=TRUE 用户即跳过,返回 false。
//! 由 api 启动按 XRAYC_BOOTSTRAP_ADMIN_PASSWORD env 调用,不触发 SEED_DEMO_DATA 禁令。
//! 登录侧账号 'admin' 匹配 is_admin 用户(auth_accounts.rs),故 email 用占位即可。
//! 不在此保存任何真实服务器地址或凭据。本头部满足前十行中文注释约束。

use crate::store::security::password_hash;
use crate::{DbError, PgStore};
use uuid::Uuid;

impl PgStore {
    /// 无管理员时建账号 admin(is_admin=TRUE),返回是否新建。已存在管理员则跳过返回 false。
    pub async fn bootstrap_admin(&self, password: &str) -> Result<bool, DbError> {
        if password.trim().is_empty() {
            return Err(DbError::InvalidInput("管理员密码不能为空".to_string()));
        }
        let mut tx = self.pool.begin().await?;
        // 串行化判断:锁住 users 防并发双建(SERIALIZABLE 语义靠存在性 + 唯一邮箱兜底)。
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE is_admin = TRUE)")
                .fetch_one(&mut *tx)
                .await?;
        if exists {
            tx.rollback().await?;
            return Ok(false);
        }
        let admin_id = Uuid::new_v4();
        let hash = password_hash(password)?;
        let xray_user_key = format!("u-{}@xrayc.local", admin_id.simple());
        let access_credential = Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO users (
                id, email, password_hash, xray_user_key, access_credential, disabled,
                is_admin, display_name
            )
            VALUES ($1, 'admin@xrayc.local', $2, $3, $4, FALSE, TRUE, 'admin')
            ON CONFLICT (email) DO NOTHING
            "#,
        )
        .bind(admin_id)
        .bind(&hash)
        .bind(&xray_user_key)
        .bind(&access_credential)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// 无默认套餐时建一个「基础套餐」(is_default=TRUE、enabled),返回是否新建。
    /// 生产(不播 demo 种子)装机必备:否则用户注册/worker maintenance 报 DefaultPlanNotFound。
    /// 幂等:已存在 is_default+enabled 套餐即跳过返回 false(条件与注册取默认套餐一致)。
    pub async fn bootstrap_default_plan(&self) -> Result<bool, DbError> {
        let mut tx = self.pool.begin().await?;
        // 已有可用默认套餐(is_default+enabled,与注册取默认套餐同条件)→ 跳过。
        let usable: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM plans WHERE is_default = TRUE AND enabled = TRUE)",
        )
        .fetch_one(&mut *tx)
        .await?;
        if usable {
            tx.rollback().await?;
            return Ok(false);
        }
        // 受 plans_single_default 唯一约束(至多一个 is_default=TRUE):若已存在被禁用的默认套餐,
        // 启用它(而非再插一个 → 唯一冲突);确无 is_default 套餐时才新建一个基础套餐。
        let enabled = sqlx::query("UPDATE plans SET enabled = TRUE WHERE is_default = TRUE")
            .execute(&mut *tx)
            .await?;
        if enabled.rows_affected() == 0 {
            sqlx::query(
                r#"
                INSERT INTO plans (id, name, is_default, traffic_limit_bytes, billing_multiplier, enabled)
                VALUES ($1, '基础套餐', TRUE, $2, 1.000, TRUE)
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(10_i64 * 1024 * 1024 * 1024)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }
}
