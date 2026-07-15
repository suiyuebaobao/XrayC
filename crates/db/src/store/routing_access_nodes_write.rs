//! 中转节点写入接口模块。
//! 这里承载后台创建和重绑中转节点的 PostgreSQL 写流程。
//! 模块保持原事务边界和校验语义，不改变 API 行为。
//! Agent Token 只保存哈希，不在数据库中保留明文。
//! 节点不再保存运行内核字段，统一只跑 xray。
//! public_port 为零时按无效输入拒绝。
//! 备注字段使用后台文本长度限制。
//! 本文件由 routing_resources 拆分而来，避免资源模块膨胀。
//! 创建与重绑共用统一的 INSERT/UPSERT 流程。
//! 本头部满足前十行中文注释约束。

use super::runtime_helpers::*;
use super::security::*;
use crate::*;
use uuid::Uuid;

impl PgStore {
    pub async fn create_admin_access_node(
        &self,
        input: AdminAccessNodeInput,
    ) -> Result<Uuid, DbError> {
        self.insert_admin_access_node(None, input).await
    }

    pub async fn create_admin_access_node_with_id(
        &self,
        access_node_id: Uuid,
        input: AdminAccessNodeInput,
    ) -> Result<Uuid, DbError> {
        self.insert_admin_access_node(Some(access_node_id), input)
            .await
    }

    pub async fn rebind_admin_installed_access_node(
        &self,
        access_node_id: Uuid,
        input: AdminAccessNodeInput,
    ) -> Result<Uuid, DbError> {
        let name = required_admin_text(&input.name, "中转节点名称", 128)?;
        let public_host = required_admin_text(&input.public_host, "中转节点地址", 255)?;
        if input.public_port == 0 {
            return Err(DbError::InvalidInput(
                "中转节点端口必须在 1-65535 之间".to_string(),
            ));
        }
        let agent_token = required_admin_text(&input.agent_token, "Agent Token", 255)?;
        let remark = optional_admin_text(&input.remark, 512);
        // 节点多模式 CF 字段:空白归一为 NULL,重绑时一并刷新对外身份。
        let cert_domain = optional_node_domain(&input.cert_domain, 255);
        let acme_email = optional_node_domain(&input.acme_email, 255);
        let cf_domain = optional_node_domain(&input.cf_domain, 255);
        let ip_direct_address = optional_node_domain(&input.ip_direct_address, 255);
        // cf_enabled 以 cf_domain 派生为准,忽略入参;cf_cert_mode 节点列一律默认 reuse_direct(token-less 免 token、
        // CF 复用直连证书);acme 不再当 dns01 判据(它是直连 HTTP-01 也要用的邮箱),要 dns01 由运维在 cf 域名上显式设。
        let cf_enabled = cf_domain.is_some();
        let cf_cert_mode = derived_cf_cert_mode(&cf_domain, &acme_email);
        // 重绑同样要把单字段域名同步进 node_domains 主行(下方 .bind() 会消费这三个值,先留存)。
        // cf_enabled 读模型按 node_domains 派生,缺这一步重绑后 CF 节点 cf_enabled 读为 false、CF 身份丢失。
        let cert_domain_value = cert_domain.clone();
        let cf_domain_value = cf_domain.clone();
        let acme_email_value = acme_email.clone();
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO access_nodes (
                id, name, public_host, public_port, agent_token_hash, remark,
                cert_domain, acme_email, cf_enabled, cf_domain, ip_direct_address, cf_cert_mode,
                config_dirty, config_dirty_at, config_dirty_reason, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                TRUE, now(), 'admin_created_access_node', 'unknown')
            ON CONFLICT (id) DO UPDATE
            SET name = EXCLUDED.name,
                public_host = EXCLUDED.public_host,
                public_port = EXCLUDED.public_port,
                agent_token_hash = EXCLUDED.agent_token_hash,
                remark = EXCLUDED.remark,
                cert_domain = EXCLUDED.cert_domain,
                acme_email = EXCLUDED.acme_email,
                cf_enabled = EXCLUDED.cf_enabled,
                cf_domain = EXCLUDED.cf_domain,
                ip_direct_address = EXCLUDED.ip_direct_address,
                cf_cert_mode = EXCLUDED.cf_cert_mode,
                config_dirty = TRUE,
                config_dirty_at = now(),
                desired_config_hash = NULL,
                applied_config_hash = NULL,
                last_heartbeat_at = NULL,
                status = 'unknown',
                agent_version = '',
                last_traffic_report_at = NULL,
                last_traffic_success_at = NULL,
                config_dirty_reason = 'admin_rebound_installed_access_node'
            RETURNING id
            "#,
        )
        .bind(access_node_id)
        .bind(name)
        .bind(public_host)
        .bind(i32::from(input.public_port))
        .bind(agent_token_hash(&agent_token))
        .bind(remark)
        .bind(cert_domain)
        .bind(acme_email)
        .bind(cf_enabled)
        .bind(cf_domain)
        .bind(ip_direct_address)
        .bind(cf_cert_mode)
        .fetch_one(&self.pool)
        .await?;
        // 与 create 路径口径一致:把节点单字段域名同步进 node_domains 主行(多域名表为准)。
        self.sync_primary_node_domains(
            id,
            cert_domain_value.as_deref(),
            cf_domain_value.as_deref(),
            acme_email_value.as_deref(),
        )
        .await?;
        Ok(id)
    }

    async fn insert_admin_access_node(
        &self,
        access_node_id: Option<Uuid>,
        input: AdminAccessNodeInput,
    ) -> Result<Uuid, DbError> {
        let name = required_admin_text(&input.name, "中转节点名称", 128)?;
        let public_host = required_admin_text(&input.public_host, "中转节点地址", 255)?;
        if input.public_port == 0 {
            return Err(DbError::InvalidInput(
                "中转节点端口必须在 1-65535 之间".to_string(),
            ));
        }
        let agent_token = required_admin_text(&input.agent_token, "Agent Token", 255)?;
        let remark = optional_admin_text(&input.remark, 512);
        // 节点多模式 CF 字段:空白归一为 NULL,落库前统一裁剪。
        let cert_domain = optional_node_domain(&input.cert_domain, 255);
        let acme_email = optional_node_domain(&input.acme_email, 255);
        let cf_domain = optional_node_domain(&input.cf_domain, 255);
        let ip_direct_address = optional_node_domain(&input.ip_direct_address, 255);
        // cf_enabled 以 cf_domain 派生为准,忽略入参;cf_cert_mode 节点列一律默认 reuse_direct(token-less 免 token、
        // CF 复用直连证书);acme 不再当 dns01 判据(它是直连 HTTP-01 也要用的邮箱),要 dns01 由运维在 cf 域名上显式设。
        let cf_enabled = cf_domain.is_some();
        let cf_cert_mode = derived_cf_cert_mode(&cf_domain, &acme_email);
        // 同步进 node_domains 前先留存域名值(下方 .bind() 会消费 cert_domain/cf_domain/acme_email)。
        let cert_domain_value = cert_domain.clone();
        let cf_domain_value = cf_domain.clone();
        let acme_email_value = acme_email.clone();
        let id = if let Some(access_node_id) = access_node_id {
            sqlx::query_scalar::<_, Uuid>(
                r#"
                INSERT INTO access_nodes (
                    id, name, public_host, public_port, agent_token_hash, remark,
                    cert_domain, acme_email, cf_enabled, cf_domain, ip_direct_address, cf_cert_mode,
                    config_dirty, config_dirty_at, config_dirty_reason, status
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                    TRUE, now(), 'admin_created_access_node', 'unknown')
                RETURNING id
                "#,
            )
            .bind(access_node_id)
            .bind(name)
            .bind(public_host)
            .bind(i32::from(input.public_port))
            .bind(agent_token_hash(&agent_token))
            .bind(remark)
            .bind(cert_domain)
            .bind(acme_email)
            .bind(cf_enabled)
            .bind(cf_domain)
            .bind(ip_direct_address)
            .bind(cf_cert_mode)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, Uuid>(
                r#"
                INSERT INTO access_nodes (
                    name, public_host, public_port, agent_token_hash, remark,
                    cert_domain, acme_email, cf_enabled, cf_domain, ip_direct_address, cf_cert_mode,
                    config_dirty, config_dirty_at, config_dirty_reason, status
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                    TRUE, now(), 'admin_created_access_node', 'unknown')
                RETURNING id
                "#,
            )
            .bind(name)
            .bind(public_host)
            .bind(i32::from(input.public_port))
            .bind(agent_token_hash(&agent_token))
            .bind(remark)
            .bind(cert_domain)
            .bind(acme_email)
            .bind(cf_enabled)
            .bind(cf_domain)
            .bind(ip_direct_address)
            .bind(cf_cert_mode)
            .fetch_one(&self.pool)
            .await?
        };
        // 节点单字段域名同步进 node_domains 主行(多域名表为准,单字段为快捷指针)。
        self.sync_primary_node_domains(
            id,
            cert_domain_value.as_deref(),
            cf_domain_value.as_deref(),
            acme_email_value.as_deref(),
        )
        .await?;
        Ok(id)
    }

    /// 把节点 cert_domain(direct)/cf_domain(cf)同步为对应 kind 的 is_primary 主行。
    /// upsert 语义:有值则插入或更新主行;cf 行的 cf_cert_mode 按是否给 acme/token 派生(dns01/reuse_direct)。
    /// 单字段清空(None)时不删旧主行,留给显式 delete_node_domain 处理引用关系,避免悬挂入口/出口。
    pub(crate) async fn sync_primary_node_domains(
        &self,
        node_id: Uuid,
        cert_domain: Option<&str>,
        cf_domain: Option<&str>,
        acme_email: Option<&str>,
    ) -> Result<(), DbError> {
        if let Some(domain) = cert_domain.filter(|d| !d.trim().is_empty()) {
            upsert_primary_node_domain(&self.pool, node_id, domain, "direct", None, acme_email)
                .await?;
        }
        if let Some(domain) = cf_domain.filter(|d| !d.trim().is_empty()) {
            // CF 主行证书模式一律 reuse_direct(token-less 免 token,与节点列派生、agent 默认一致)。
            // acme 是直连证书 HTTP-01 也要用的邮箱、不是 DNS-01 凭据信号(那需 CF API token、且 token 不持久化),
            // 故不再「有 acme→dns01」误判;要 dns01 由运维在 cf 域名上显式传 cf_cert_mode,经 add_node_domain 落库。
            upsert_primary_node_domain(
                &self.pool,
                node_id,
                domain,
                "cf",
                Some("reuse_direct"),
                acme_email,
            )
            .await?;
        }
        Ok(())
    }
}

/// upsert 某节点某 kind 的 is_primary 主域名行:先清同 kind 旧 is_primary,再插入或更新目标行为主。
/// 用 ON CONFLICT(access_node_id,domain) 兼容「同名域名改 kind/模式」与重复写入幂等。
async fn upsert_primary_node_domain(
    pool: &sqlx::PgPool,
    node_id: Uuid,
    domain: &str,
    kind: &str,
    cf_cert_mode: Option<&str>,
    acme_email: Option<&str>,
) -> Result<(), DbError> {
    let domain: String = domain.trim().chars().take(255).collect();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE node_domains SET is_primary = FALSE WHERE access_node_id = $1 AND kind = $2",
    )
    .bind(node_id)
    .bind(kind)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO node_domains (access_node_id, domain, kind, cf_cert_mode, acme_email, is_primary)
        VALUES ($1, $2, $3, $4, $5, TRUE)
        ON CONFLICT (access_node_id, domain) DO UPDATE
        SET kind = EXCLUDED.kind,
            cf_cert_mode = EXCLUDED.cf_cert_mode,
            acme_email = EXCLUDED.acme_email,
            is_primary = TRUE
        "#,
    )
    .bind(node_id)
    .bind(&domain)
    .bind(kind)
    .bind(cf_cert_mode)
    .bind(acme_email)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// 派生节点 CF 证书模式,口径与 sync_primary_node_domains / routing_node_domains::derive_cf_cert_mode 一致:
/// 节点列一律默认 reuse_direct(token-less 免 token)——CF 入口复用直连灰云证书(CF Full 非 strict 回源),
/// 订阅公布 cf_domain 藏源站 IP,这才是「CF 就配成 CF、免 token」的正路,不再把 CF 域名当直连域名绕。
/// 关键修正:acme 邮箱是直连证书 HTTP-01 也要用的邮箱,不是 DNS-01 凭据信号(DNS-01 给 cf_domain 签证书需
/// CF API token、且 token 不持久化),旧实现「有 cf_domain 且有 acme→dns01」会把「有直连证书的 CF 节点」误判成
/// dns01 → 锚到签不出的 cf_domain DNS-01 路径、入口被 BUG-D 跳过。要 dns01 须由运维在 cf 域名上显式设
/// cf_cert_mode='dns01'(经 add_node_domain 落 node_domains 行),不再由 acme 自动推断。
fn derived_cf_cert_mode(_cf_domain: &Option<String>, _acme_email: &Option<String>) -> String {
    // 节点列 cf_cert_mode 一律默认 reuse_direct(token-less 免 token):CF 入口复用直连灰云证书、
    // CF Full 非 strict 回源、订阅公布 cf_domain 藏源站 IP。acme 是直连证书 HTTP-01 也要用的邮箱、
    // 不是 DNS-01 凭据信号(DNS-01 给 cf_domain 签证书需 CF API token,且 token 不持久化),故不再
    // 「有 acme→dns01」误判;要 dns01 由运维在 cf 域名上显式设 cf_cert_mode='dns01'(走 node_domains 行)。
    "reuse_direct".to_string()
}

/// 节点域名/邮箱类字段归一:裁剪空白,空串转 None,超长截断到 max。
fn optional_node_domain(value: &Option<String>, max: usize) -> Option<String> {
    value.as_ref().and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.chars().take(max).collect())
        }
    })
}
