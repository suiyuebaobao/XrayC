//! 入口选中域名(node_domain)解析与护栏概要。
//! 从 routing_access_entries 拆出,避免主文件超过 550 行硬上限。
//! 多域名 Phase 2:入口可选一个 node_domain,护栏据其 kind(direct/cf)判定协议组合。
//! 解析时校验该域名存在且属于本节点,避免选别节点域名绕过护栏。
//! 不选(None)时返回默认概要,护栏回退节点单域名(cdn_enabled/server_name)现有行为。
//! 本模块在事务内查询 node_domains,不访问远端、不改写其他表。
//! kind 借用 holder 持有的 String,避免把所有权塞进 PreparedAccessEntry。
//! 由 routing_access_entries / routing_access_entry_updates 以 super:: 调用。
//! 注释保持中文,满足仓库拆分约束。
//! 本头部满足前十行中文注释约束。

use crate::DbError;
use uuid::Uuid;

/// 入口选中的 node_domain 概要(多域名 Phase 2/3):kind(direct/cf)、domain 与是否选了域名。
/// None / has_domain=false 表示不选——护栏与证书锚定都回退节点单域名(cdn_enabled / server_name / cert_domain)。
/// 选了域名时:护栏据 kind 判定;证书路径据 domain + kind 锚定选中域名(direct 锚自己、cf 锚 cf 域名自己的证书)。
#[derive(Clone, Copy, Default)]
pub(crate) struct SelectedEntryDomain<'a> {
    pub(crate) kind: Option<&'a str>,
    pub(crate) domain: Option<&'a str>,
    pub(crate) has_domain: bool,
}

/// 选中域名的拥有式持有者:在事务内查出后持有 String,借出 SelectedEntryDomain 借用视图。
/// 把 kind/domain 集中持有,避免调用方为每个字段单独维护 holder 变量。
#[derive(Default)]
pub(crate) struct SelectedEntryDomainHolder {
    pub(crate) kind: Option<String>,
    pub(crate) domain: Option<String>,
}

impl SelectedEntryDomainHolder {
    /// 借出借用视图:has_domain 由 domain 是否解析出来表达。
    pub(crate) fn view(&self) -> SelectedEntryDomain<'_> {
        SelectedEntryDomain {
            kind: self.kind.as_deref(),
            domain: self.domain.as_deref(),
            has_domain: self.domain.is_some(),
        }
    }
}

/// 在事务内解析入口选中的 node_domain,把 kind/domain 填进 holder 并返回借用视图。
///
/// 校验该域名存在且属于本节点(access_node_id 比对),避免选了别节点的域名绕过护栏/锚错证书。
/// 不选(None)时 holder 保持空,返回默认视图(has_domain=false),护栏与证书锚定都回退节点单域名行为。
/// holder 由调用方持有,生命周期与返回的视图一致(证书锚定要 domain/kind)。
pub(crate) async fn resolve_selected_entry_domain_in_tx<'a>(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    access_node_id: Uuid,
    node_domain_id: Option<Uuid>,
    holder: &'a mut SelectedEntryDomainHolder,
) -> Result<SelectedEntryDomain<'a>, DbError> {
    let Some(domain_id) = node_domain_id else {
        return Ok(SelectedEntryDomain::default());
    };
    let row = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT access_node_id, kind, domain FROM node_domains WHERE id = $1",
    )
    .bind(domain_id)
    .fetch_optional(&mut **tx)
    .await?;
    match row {
        Some((owner_id, _, _)) if owner_id != access_node_id => Err(DbError::InvalidInput(
            "选中的域名不属于该中转节点,请重新选择本节点的域名".to_string(),
        )),
        Some((_, kind, domain)) => {
            holder.kind = Some(kind);
            holder.domain = Some(domain);
            Ok(holder.view())
        }
        None => Err(DbError::InvalidInput(
            "选中的节点域名不存在,请刷新后重新选择".to_string(),
        )),
    }
}
