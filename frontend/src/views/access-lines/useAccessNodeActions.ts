// 本文件封装中转节点卡片级动作。
// 它负责节点新增、编辑、删除和批量删除。
// 页面组合层只消费这里返回的状态和事件函数。
// 删除动作只删除控制台记录，不主动清理远端服务器。
// 注释使用中文，满足仓库前十行注释约束。

import { ElMessage, ElMessageBox } from 'element-plus';
import { ref, type Ref } from 'vue';
import { apiClient, type AccessNodeSummary, type NodeDomainInput } from '@/services/api';
import { emptyNodeForm } from '@/views/access-lines/forms';
import type { NodeDomainFormRow, NodeForm, RelayNodeView } from '@/views/access-lines/types';

type NodeActionContext = {
  saving: Ref<boolean>;
  nodes: Ref<AccessNodeSummary[]>;
  nodeDialogOpen: Ref<boolean>;
  nodeForm: Ref<NodeForm>;
  load: () => Promise<void>;
};

export function useAccessNodeActions(context: NodeActionContext) {
  const selectedNodeIds = ref<string[]>([]);
  const editingNode = ref<RelayNodeView | null>(null);
  const nodeDialogMode = ref<'create' | 'edit'>('create');
  // 正在提交重启请求的节点 id：卡片按它展示「重启中」loading，空串表示无进行中重启。
  const rebootingNodeId = ref('');

  function openCreateNodeDialog() {
    editingNode.value = null;
    nodeDialogMode.value = 'create';
    context.nodeForm.value = emptyNodeForm();
    context.nodeDialogOpen.value = true;
  }

  function openEditNodeDialog(node: RelayNodeView) {
    editingNode.value = node;
    nodeDialogMode.value = 'edit';
    context.nodeForm.value = {
      name: node.name,
      publicPort: node.publicPort || 443,
      sshHost: node.sshHost,
      agentToken: '',
      remark: node.remark,
      ipDirectAddress: node.ipDirectAddress ?? '',
      domains: nodeDomainsToForm(node),
      acmeEmail: node.acmeEmail ?? '',
    };
    context.nodeDialogOpen.value = true;
  }

  async function saveRelayNode() {
    context.saving.value = true;
    try {
      const ipDirectAddress = context.nodeForm.value.ipDirectAddress.trim();
      const acmeEmail = context.nodeForm.value.acmeEmail.trim();
      // 收敛多域名列表（去空、查重、确保每组有主域名）。
      const domains = collectNodeDomains(context.nodeForm.value.domains);
      const directDomains = domains.filter((row) => row.kind === 'direct');
      const cfDomains = domains.filter((row) => row.kind === 'cf');
      // 至少填一个对外直连地址（IP / 域名 / CF），否则节点无法对外提供入口。
      if (!ipDirectAddress && domains.length === 0) {
        throw new Error('请至少填写一个对外直连地址（IP 直连 / 域名直连 / CF 直连）');
      }
      // 填了任意域名（直连或 CF）就要签证书，必须有 ACME 邮箱；CF 启用由后端按是否有 cf 域名派生。
      if (domains.length > 0 && !acmeEmail) {
        throw new Error('填写域名直连或 CF 直连地址时请填写 ACME 邮箱');
      }
      // 旧单域名字段保留兼容：取各组主域名回填 cert_domain/cf_domain，后端 Phase 6 同时读 domains。
      const primaryDirect = directDomains.find((row) => row.is_primary) ?? directDomains[0];
      const primaryCf = cfDomains.find((row) => row.is_primary) ?? cfDomains[0];
      const payload = {
        name: context.nodeForm.value.name.trim(),
        // 客户连接地址不再单独填写:由对外直连地址按 IP直连>直连域名>CF域名 推导
        // (优先直连 IP/灰云,避开拿 CF 橙云域名当主机的 Bug④ 老坑)。
        public_host: ipDirectAddress || primaryDirect?.domain || primaryCf?.domain || '',
        public_port: normalizeNodePort(context.nodeForm.value.publicPort),
        ssh_host: context.nodeForm.value.sshHost.trim(),
        remark: context.nodeForm.value.remark.trim(),
        ip_direct_address: ipDirectAddress,
        cert_domain: primaryDirect?.domain ?? '',
        acme_email: acmeEmail,
        cf_domain: primaryCf?.domain ?? '',
        domains,
      };
      if (editingNode.value) {
        await apiClient.updateAccessNode(editingNode.value.id, payload);
      } else {
        const agentToken = context.nodeForm.value.agentToken.trim();
        if (!agentToken) {
          throw new Error('请填写鉴权码');
        }
        await apiClient.createAccessNode({ ...payload, agent_token: agentToken });
      }
      context.nodeDialogOpen.value = false;
      await context.load();
      ElMessage.success(editingNode.value ? '中转节点已更新' : '中转节点已创建');
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '保存中转节点失败');
    } finally {
      context.saving.value = false;
    }
  }

  function toggleNodeSelection(node: RelayNodeView, checked: boolean) {
    const ids = new Set(selectedNodeIds.value);
    if (checked) {
      ids.add(node.id);
    } else {
      ids.delete(node.id);
    }
    selectedNodeIds.value = Array.from(ids);
  }

  async function deleteRelayNode(node: RelayNodeView) {
    await deleteNodeIds([node.id], `确认删除中转节点“${node.name}”？`);
  }

  // 提交节点重启请求：二次确认警告整机停机，确认后只下发请求，由 agent 带安全自检后重启宿主。
  async function rebootRelayNode(node: RelayNodeView) {
    try {
      await ElMessageBox.confirm(
        `重启会导致中转节点“${node.name}”【整机重启】、期间该节点上所有入口【停止服务】数分钟，确认重启？`,
        '重启中转节点',
        {
          type: 'warning',
          confirmButtonText: '确认重启',
          cancelButtonText: '取消',
        },
      );
      rebootingNodeId.value = node.id;
      await apiClient.rebootAccessNode(node.id);
      await context.load();
      ElMessage.success('已提交重启请求，节点将在下次心跳后由 agent 带安全自检执行重启');
    } catch (error) {
      if (error === 'cancel') {
        return;
      }
      ElMessage.error(error instanceof Error ? error.message : '提交重启请求失败');
    } finally {
      rebootingNodeId.value = '';
    }
  }

  async function deleteSelectedRelayNodes() {
    await deleteNodeIds(selectedNodeIds.value, `确认删除选中的 ${selectedNodeIds.value.length} 个中转节点？`, true);
  }

  async function deleteNodeIds(ids: string[], message: string, forceBatchEndpoint = false) {
    if (ids.length === 0) {
      ElMessage.warning('请先选择中转节点');
      return;
    }
    try {
      await ElMessageBox.confirm(`${message} 只删除控制台节点和从属连接点，不清理远端机器。`, '删除中转节点', {
        type: 'warning',
        confirmButtonText: '确认',
        cancelButtonText: '取消',
      });
      context.saving.value = true;
      const result = ids.length === 1 && !forceBatchEndpoint
        ? await apiClient.deleteAccessNode(ids[0])
        : await apiClient.batchDeleteAccessNodes(ids);
      selectedNodeIds.value = selectedNodeIds.value.filter((id) => !ids.includes(id));
      await context.load();
      ElMessage.success(`已删除 ${result.deletedNodeCount} 个节点、${result.deletedAccessLineCount} 个入口`);
    } catch (error) {
      if (error === 'cancel') {
        return;
      }
      ElMessage.error(error instanceof Error ? error.message : '删除中转节点失败');
    } finally {
      context.saving.value = false;
    }
  }

  return {
    selectedNodeIds,
    nodeDialogMode,
    rebootingNodeId,
    openCreateNodeDialog,
    openEditNodeDialog,
    saveRelayNode,
    toggleNodeSelection,
    deleteRelayNode,
    deleteSelectedRelayNodes,
    rebootRelayNode,
  };
}

function normalizeNodePort(value: number) {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error('中转节点端口必须在 1-65535 之间');
  }
  return port;
}

// 把节点读模型的 domains[] 映射成表单多行；读模型没出 domains 时按旧单域名兜底拆成行。
function nodeDomainsToForm(node: AccessNodeSummary): NodeDomainFormRow[] {
  if (node.domains && node.domains.length > 0) {
    return node.domains.map((domain) => ({
      domain: domain.domain,
      kind: domain.kind,
      isPrimary: domain.isPrimary,
      cfApiToken: '',
    }));
  }
  // 兜底：用旧单域名字段拆出一组直连 + 一组 CF（编辑老节点也能多域名继续维护）。
  const rows: NodeDomainFormRow[] = [];
  if (node.certDomain?.trim()) {
    rows.push({ domain: node.certDomain.trim(), kind: 'direct', isPrimary: true, cfApiToken: '' });
  }
  if (node.cfDomain?.trim()) {
    rows.push({ domain: node.cfDomain.trim(), kind: 'cf', isPrimary: true, cfApiToken: '' });
  }
  return rows;
}

// 收敛表单多域名：去空白、同（kind+域名）查重、确保每组恰有一个主域名后转成提交载荷。
function collectNodeDomains(rows: NodeDomainFormRow[]): NodeDomainInput[] {
  const seen = new Set<string>();
  const cleaned = rows
    .map((row) => ({ domain: row.domain.trim(), kind: row.kind, is_primary: row.isPrimary }))
    .filter((row) => {
      if (!row.domain) {
        return false;
      }
      const key = `${row.kind}:${row.domain.toLowerCase()}`;
      if (seen.has(key)) {
        throw new Error(`域名「${row.domain}」在同一类型下重复，请删除多余行`);
      }
      seen.add(key);
      return true;
    });
  // 每组（direct/cf）确保恰有一个主域名：缺则第一行顶上，多则只留第一个。
  (['direct', 'cf'] as const).forEach((kind) => {
    const group = cleaned.filter((row) => row.kind === kind);
    if (group.length === 0) {
      return;
    }
    let primaryAssigned = false;
    group.forEach((row) => {
      if (row.is_primary && !primaryAssigned) {
        primaryAssigned = true;
      } else {
        row.is_primary = false;
      }
    });
    if (!primaryAssigned) {
      group[0].is_primary = true;
    }
  });
  return cleaned;
}
