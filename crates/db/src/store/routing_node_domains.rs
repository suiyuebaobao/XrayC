//! 中转节点多域名(node_domains)增删查模块。
//! 这里承载某节点的直连/CF 域名清单读写,以及护栏/cf_enabled 的派生查询。
//! 域名校验:domain 非空、kind∈{direct,cf}、同节点 domain 唯一。
//! cf 域名的 cf_cert_mode 默认 reuse_direct(token-less 免 token),仅显式传 cf_cert_mode 才覆盖。
//! set_primary 保证同 kind 下唯一 is_primary;delete 被入口/出口引用则拒绝。
//! 写入以 node_domains 为准,节点单字段列保留为主域名快捷指针(节点写入侧同步)。
//! 所有 SQL 走 self.pool,保持与其他 store 一致的错误封装。
//! 不在此引入 API、前端或脚本层行为改动。
//! 注释使用中文,满足仓库拆分约束。
//! 本头部满足前十行中文注释约束。

use crate::*;
use sqlx::FromRow;
use uuid::Uuid;

/// node_domains 行映射,字段顺序与表列一致。
#[derive(Debug, FromRow)]
struct NodeDomainRow {
    id: Uuid,
    access_node_id: Uuid,
    domain: String,
    kind: String,
    cf_cert_mode: Option<String>,
    acme_email: Option<String>,
    is_primary: bool,
    cert_status: String,
}

impl From<NodeDomainRow> for NodeDomain {
    fn from(row: NodeDomainRow) -> Self {
        NodeDomain {
            id: row.id,
            access_node_id: row.access_node_id,
            domain: row.domain,
            kind: row.kind,
            cf_cert_mode: row.cf_cert_mode,
            acme_email: row.acme_email,
            is_primary: row.is_primary,
            cert_status: row.cert_status,
        }
    }
}

impl PgStore {
    /// 列出某节点的全部域名,按 created_at 升序(回填的主域名最先,后续新增追加在后)。
    pub async fn list_node_domains(&self, node_id: Uuid) -> Result<Vec<NodeDomain>, DbError> {
        let rows = sqlx::query_as::<_, NodeDomainRow>(
            r#"
            SELECT id, access_node_id, domain, kind, cf_cert_mode, acme_email,
                   is_primary, cert_status
            FROM node_domains
            WHERE access_node_id = $1
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(NodeDomain::from).collect())
    }

    /// 按 id 取单个节点域名(护栏据此判定选中域名的 kind/domain)。
    /// 不存在返回 None;调用方再自行校验该域名是否属于目标节点(access_node_id 比对)。
    pub async fn get_node_domain(&self, domain_id: Uuid) -> Result<Option<NodeDomain>, DbError> {
        let row = sqlx::query_as::<_, NodeDomainRow>(
            r#"
            SELECT id, access_node_id, domain, kind, cf_cert_mode, acme_email,
                   is_primary, cert_status
            FROM node_domains
            WHERE id = $1
            "#,
        )
        .bind(domain_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(NodeDomain::from))
    }

    /// 新增节点域名:校验 domain 非空、kind 合法、同节点 domain 唯一;cf 派生 cf_cert_mode。
    /// is_primary=true 时先清掉同节点同 kind 其余行的 is_primary,保证唯一主域名。
    pub async fn add_node_domain(
        &self,
        node_id: Uuid,
        input: AddNodeDomainInput,
    ) -> Result<Uuid, DbError> {
        let domain = input.domain.trim();
        if domain.is_empty() {
            return Err(DbError::InvalidInput("域名不能为空".to_string()));
        }
        let domain: String = domain.chars().take(255).collect();
        if input.kind != "direct" && input.kind != "cf" {
            return Err(DbError::InvalidInput(format!(
                "节点域名类型无效: {}(仅支持 direct/cf)",
                input.kind
            )));
        }
        let acme_email = normalize_optional(&input.acme_email, 255);
        // cf 域名的证书模式派生:显式给了 cf_cert_mode 用之,否则一律 reuse_direct(token-less 免 token)。
        // direct 域名不需要 cf_cert_mode,统一存 NULL。
        let cf_cert_mode = if input.kind == "cf" {
            Some(derive_cf_cert_mode(&input.cf_cert_mode, &acme_email))
        } else {
            None
        };

        let mut tx = self.pool.begin().await?;
        // 节点必须存在,避免悬挂外键(虽有 FK,但提前给出清晰原因)。
        let node_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM access_nodes WHERE id = $1)",
        )
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await?;
        if !node_exists {
            return Err(DbError::InvalidAgentPayload(format!(
                "中转节点不存在: {node_id}"
            )));
        }
        // 同节点 domain 唯一:提前查重给出业务原因(而非裸 unique 约束错误)。
        let dup = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM node_domains WHERE access_node_id = $1 AND domain = $2)",
        )
        .bind(node_id)
        .bind(&domain)
        .fetch_one(&mut *tx)
        .await?;
        if dup {
            return Err(DbError::InvalidInput(format!("该节点已存在域名: {domain}")));
        }
        if input.is_primary {
            clear_primary_for_kind(&mut tx, node_id, &input.kind).await?;
        }
        let id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO node_domains (access_node_id, domain, kind, cf_cert_mode, acme_email, is_primary)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
        )
        .bind(node_id)
        .bind(&domain)
        .bind(&input.kind)
        .bind(cf_cert_mode)
        .bind(acme_email)
        .bind(input.is_primary)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// 删除节点域名:若仍被 access_entries / exit_endpoints 引用则拒绝(先改引用再删)。
    pub async fn delete_node_domain(&self, domain_id: Uuid) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let referenced = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(SELECT 1 FROM access_entries WHERE node_domain_id = $1)
                OR EXISTS(SELECT 1 FROM exit_endpoints WHERE node_domain_id = $1)
            "#,
        )
        .bind(domain_id)
        .fetch_one(&mut *tx)
        .await?;
        if referenced {
            return Err(DbError::InvalidInput(
                "该域名仍被入口或本机出口引用,请先改用其他域名再删除".to_string(),
            ));
        }
        let affected = sqlx::query("DELETE FROM node_domains WHERE id = $1")
            .bind(domain_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if affected == 0 {
            return Err(DbError::InvalidInput(format!(
                "节点域名不存在: {domain_id}"
            )));
        }
        tx.commit().await?;
        Ok(())
    }

    /// 设为该 kind 下的主域名:清掉同节点同 kind 其余行的 is_primary,再把目标行置 true。
    pub async fn set_primary_node_domain(
        &self,
        node_id: Uuid,
        domain_id: Uuid,
        kind: &str,
    ) -> Result<(), DbError> {
        if kind != "direct" && kind != "cf" {
            return Err(DbError::InvalidInput(format!(
                "节点域名类型无效: {kind}(仅支持 direct/cf)"
            )));
        }
        let mut tx = self.pool.begin().await?;
        clear_primary_for_kind(&mut tx, node_id, kind).await?;
        let affected = sqlx::query(
            r#"
            UPDATE node_domains
            SET is_primary = TRUE
            WHERE id = $1 AND access_node_id = $2 AND kind = $3
            "#,
        )
        .bind(domain_id)
        .bind(node_id)
        .bind(kind)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if affected == 0 {
            return Err(DbError::InvalidInput(format!(
                "节点域名不存在或类型不匹配: {domain_id}"
            )));
        }
        tx.commit().await?;
        Ok(())
    }

    /// 节点是否存在某 kind 的域名:护栏/cf_enabled 派生用(存在 kind='cf' 行即视为启用 CF)。
    pub async fn node_has_domain_kind(&self, node_id: Uuid, kind: &str) -> Result<bool, DbError> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM node_domains WHERE access_node_id = $1 AND kind = $2)",
        )
        .bind(node_id)
        .bind(kind)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// 多域名 Phase 5b:一次性读全部 node_domains,按节点回填 StoreData 里各节点的 domains[]。
    /// 从 load_store_data 抽出,既守住 load.rs 的 550 行硬上限,又把多域名读侧落点集中本模块。
    /// 只回填已存在于 data.access_nodes 的节点;读侧契约由 AccessNode.domains 默认空数组保证必含。
    pub(crate) async fn populate_node_domains_into(
        &self,
        data: &mut StoreData,
    ) -> Result<(), DbError> {
        for row in sqlx::query_as::<_, NodeDomainViewRow>(
            r#"
            SELECT access_node_id, id, domain, kind, is_primary, cert_status
            FROM node_domains
            ORDER BY access_node_id, created_at ASC, id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            if let Some(node) = data.access_nodes.get_mut(&row.access_node_id) {
                node.domains.push(NodeDomainView {
                    id: row.id,
                    domain: row.domain,
                    kind: row.kind,
                    is_primary: row.is_primary,
                    cert_status: row.cert_status,
                });
            }
        }
        Ok(())
    }

    /// 回填分组绑定节点 id,只取仍有对应运行态 access_line 的绑定。
    ///
    /// `access_lines.id` 与 `access_entry_exit_bindings.id` 同主键(同步时 al.id = b.id),
    /// 订阅/计费侧靠 `binding_node_ids.contains(&line.id)` 对 access_lines 判定可见。
    /// `line_group_binding_nodes` 行随删除绑定有外键级联清理,但出口池剪枝/出口删除等路径会
    /// 删掉运行态 access_line 而保留 binding——残留的悬挂 id 会让分组唯一线路全不可见、订阅整单
    /// 422「没有可用线路」,且管理端读模型透出后前端反选不掉、replace 又拒失效 id。这里在读模型
    /// 源头用 EXISTS 过滤掉缺运行态线路的悬挂行,让订阅与管理端同时永不被悬挂 id 污染(防御、幂等)。
    pub(crate) async fn populate_line_group_binding_nodes_into(
        &self,
        data: &mut StoreData,
    ) -> Result<(), DbError> {
        for row in sqlx::query_as::<_, super::rows::LineGroupBindingNodeRow>(
            r#"
            SELECT lgbn.line_group_id, lgbn.entry_exit_binding_id
            FROM line_group_binding_nodes lgbn
            WHERE EXISTS (SELECT 1 FROM access_lines al WHERE al.id = lgbn.entry_exit_binding_id)
            ORDER BY lgbn.line_group_id, lgbn.position, lgbn.entry_exit_binding_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?
        {
            if let Some(group) = data.line_groups.get_mut(&row.line_group_id) {
                group.binding_node_ids.push(row.entry_exit_binding_id);
            }
        }
        Ok(())
    }
}

/// 清掉某节点某 kind 下所有行的 is_primary,供新增/设主前置。
async fn clear_primary_for_kind(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
    kind: &str,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE node_domains SET is_primary = FALSE WHERE access_node_id = $1 AND kind = $2",
    )
    .bind(node_id)
    .bind(kind)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 派生 CF 证书模式:显式给了非空 cf_cert_mode 用之;否则一律 reuse_direct(token-less 免 token)。
/// 不再用「有 acme→dns01」——acme 是直连证书 HTTP-01 也要用的邮箱、不是 DNS-01 凭据信号(DNS-01 给 cf_domain
/// 签证书需 CF API token);要 dns01 须由运维在 cf 域名上显式传 cf_cert_mode='dns01'。
fn derive_cf_cert_mode(explicit: &Option<String>, _acme_email: &Option<String>) -> String {
    if let Some(mode) = explicit {
        let trimmed = mode.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "reuse_direct".to_string()
}

/// 可空文本归一:裁剪空白,空串转 None,超长截断到 max。
fn normalize_optional(value: &Option<String>, max: usize) -> Option<String> {
    value.as_ref().and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.chars().take(max).collect())
        }
    })
}
