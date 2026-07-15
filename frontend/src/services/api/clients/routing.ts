// 本文件维护中转节点、入口管理、出口管理和分组 API 方法。
// 它只组织控制面请求和部署参数，不包含页面弹窗或表单状态。
// 管理员后台可读取出口管理真实配置；用户侧和公开输出仍不得暴露出口凭据。
// Agent 安装说明、入口出口绑定和出口管理维护都统一从这里调用。

import type {
  AccessEntryExitBindingPayload,
  AccessEntryExitBindingSummary,
  AccessEntryPayload,
  AccessEntrySummary,
  AccessNodeOneClickInstallPayload,
  AccessNodeOneClickInstallResult,
  AccessNodeRebootResult,
  AccessNodeTlsRenewResult,
  ControlPlane,
  AccessNodeDeleteResult,
  AgentInstallGuidePayload,
  AgentInstallGuideResult,
  CreateAccessNodePayload,
  CreateExitEndpointPayload,
  CreateLocalExitLinePayload,
  CreateLocalExitLinesResult,
  DeploymentTasksResult,
  CreateLineGroupPayload,
  ExitEndpointSummary,
  ExitPool,
  ExitResourceSummary,
  LineGroupBindingNodePayload,
  LocalExitLineSummary,
  NodeTrafficSummary,
  NodeTrafficTrend,
  SubscriptionRuleSet,
  SubscriptionRuleSetPayload,
  UpdateAccessNodePayload,
  UpdateExitEndpointPayload,
  UpdateLineGroupPayload,
  UpdateLocalExitLinePayload,
} from '../types';
import { request } from '../http';
import { arrayValue, createdId, isRecord, recordValue, stringValue } from '../primitives';
import {
  normalizeControlPlane,
  normalizeAccessEntry,
  normalizeAccessEntryExitBinding,
  normalizeAccessNodeOneClickInstallResult,
  normalizeAgentInstallGuideResult,
  normalizeAccessNodeRebootResult,
  normalizeAccessNodeTlsRenewResult,
  normalizeCreateLocalExitLinesResult,
  normalizeDeploymentTasksResult,
  normalizeExitEndpoint,
  normalizeExitPool,
  normalizeExitResource,
  normalizeLocalExitLineSummary,
  normalizeNodeTrafficSummary,
  normalizeNodeTrafficTrend,
  normalizeSubscriptionRuleSet,
} from '../normalizers/routing';
import { rustApiPaths } from '../paths';

export const routingApi = {
  async getAccessLines() {
    const data = await this.getControlPlane();
    return data.accessLines;
  },

  async listAccessEntries(): Promise<AccessEntrySummary[]> {
    const response = await request<unknown>(rustApiPaths.accessEntries);
    const data = recordValue(response);
    return arrayValue(data.entries ?? data.access_entries ?? data.items ?? response).map(normalizeAccessEntry);
  },

  async createAccessEntry(payload: AccessEntryPayload) {
    return createdId(await request<Record<string, unknown>>(rustApiPaths.accessEntries, {
      method: 'POST',
      body: JSON.stringify(payload),
    }));
  },

  async updateAccessEntry(accessEntryId: string, payload: Partial<AccessEntryPayload>) {
    await request<Record<string, unknown>>(`${rustApiPaths.accessEntries}/${accessEntryId}`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  },

  async deleteAccessEntry(accessEntryId: string) {
    await request<Record<string, unknown>>(`${rustApiPaths.accessEntries}/${accessEntryId}`, {
      method: 'DELETE',
    });
  },

  async listAccessEntryExitBindings(): Promise<AccessEntryExitBindingSummary[]> {
    const response = await request<unknown>(rustApiPaths.accessEntryExitBindings);
    const data = recordValue(response);
    return arrayValue(data.bindings ?? data.access_entry_exit_bindings ?? data.items ?? response)
      .map(normalizeAccessEntryExitBinding);
  },

  async createAccessEntryExitBinding(accessEntryId: string, payload: AccessEntryExitBindingPayload) {
    return createdId(await request<Record<string, unknown>>(`${rustApiPaths.accessEntries}/${accessEntryId}/exit-bindings`, {
      method: 'POST',
      body: JSON.stringify(payload),
    }));
  },

  async updateAccessEntryExitBinding(bindingId: string, payload: Partial<AccessEntryExitBindingPayload>) {
    await request<Record<string, unknown>>(`${rustApiPaths.accessEntryExitBindings}/${bindingId}`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  },

  async deleteAccessEntryExitBinding(bindingId: string) {
    await request<Record<string, unknown>>(`${rustApiPaths.accessEntryExitBindings}/${bindingId}`, {
      method: 'DELETE',
    });
  },

  async getControlPlane(): Promise<ControlPlane> {
    const data = await request<Record<string, unknown>>(rustApiPaths.accessRouting);
    return normalizeControlPlane(data);
  },

  // 监控中心·节点业务流量汇总：某区间内每个节点上/下/总字节，面板每行「今日」量用。
  async getNodeTraffic(fromMs: number, toMs: number): Promise<NodeTrafficSummary[]> {
    const params = new URLSearchParams();
    params.set('from_ms', String(fromMs));
    params.set('to_ms', String(toMs));
    const response = await request<unknown>(`${rustApiPaths.monitorNodeTraffic}?${params.toString()}`);
    const data = recordValue(response);
    return arrayValue(data.nodes ?? data.items ?? response).map(normalizeNodeTrafficSummary);
  },

  // 监控中心·单节点流量趋势：按 bucket（hour/day）分桶的趋势点 + 区间汇总，详情弹窗用。
  async getNodeTrafficTrend(
    nodeId: string,
    fromMs: number,
    toMs: number,
    bucket: 'hour' | 'day',
  ): Promise<NodeTrafficTrend> {
    const params = new URLSearchParams();
    params.set('from_ms', String(fromMs));
    params.set('to_ms', String(toMs));
    params.set('bucket', bucket);
    const response = await request<unknown>(
      `${rustApiPaths.monitorNodeTraffic}/${nodeId}/trend?${params.toString()}`,
    );
    return normalizeNodeTrafficTrend(response);
  },

  async getExitPools(): Promise<ExitPool[]> {
    const data = await request<unknown>(rustApiPaths.exitPools);
    return arrayValue(isRecord(data) ? (data.exitPools ?? data.exit_pools ?? data.items) : data).map((pool) =>
      normalizeExitPool(pool),
    );
  },

  async getExitResources(): Promise<ExitResourceSummary[]> {
    const data = await request<unknown>(rustApiPaths.exitResources);
    return arrayValue(isRecord(data) ? (data.exitResources ?? data.exit_resources ?? data.items) : data).map((item) =>
      normalizeExitResource(item),
    );
  },

  async getExitEndpoints(): Promise<ExitEndpointSummary[]> {
    const data = await request<unknown>(rustApiPaths.exitEndpoints);
    return arrayValue(isRecord(data) ? (data.exitEndpoints ?? data.exit_endpoints ?? data.items) : data).map((item) =>
      normalizeExitEndpoint(item),
    );
  },

  async createAccessNode(payload: CreateAccessNodePayload) {
    return createdId(
      await request<Record<string, unknown>>(rustApiPaths.accessNodes, {
        method: 'POST',
        body: JSON.stringify(payload),
      }),
    );
  },

  async updateAccessNode(accessNodeId: string, payload: UpdateAccessNodePayload) {
    await request<Record<string, unknown>>(`${rustApiPaths.accessNodes}/${accessNodeId}`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  },

  async deleteAccessNode(accessNodeId: string): Promise<AccessNodeDeleteResult> {
    return normalizeAccessNodeDeleteResult(await request<Record<string, unknown>>(`${rustApiPaths.accessNodes}/${accessNodeId}`, {
      method: 'DELETE',
    }));
  },

  async batchDeleteAccessNodes(accessNodeIds: string[]): Promise<AccessNodeDeleteResult> {
    return normalizeAccessNodeDeleteResult(await request<Record<string, unknown>>(`${rustApiPaths.accessNodes}/batch-delete`, {
      method: 'POST',
      body: JSON.stringify({ access_node_ids: accessNodeIds }),
    }));
  },

  async renewAccessNodeTls(accessNodeId: string): Promise<AccessNodeTlsRenewResult> {
    const data = await request<unknown>(`${rustApiPaths.accessNodes}/${accessNodeId}/tls/renew`, {
      method: 'POST',
      body: JSON.stringify({}),
    });
    return normalizeAccessNodeTlsRenewResult(recordValue(data).data ?? data);
  },

  // 给节点记一个重启请求：经心跳下发给 agent 带安全自检后重启宿主，api 侧不直接 SSH。
  async rebootAccessNode(accessNodeId: string): Promise<AccessNodeRebootResult> {
    const data = await request<unknown>(`${rustApiPaths.accessNodes}/${accessNodeId}/reboot`, {
      method: 'POST',
      body: JSON.stringify({}),
    });
    return normalizeAccessNodeRebootResult(recordValue(data).data ?? data);
  },

  async createAgentInstallGuide(payload: AgentInstallGuidePayload): Promise<AgentInstallGuideResult> {
    const data = await request<unknown>(rustApiPaths.accessNodeInstallGuide, {
      method: 'POST',
      body: JSON.stringify({
        access_node_id: payload.accessNodeId || undefined,
        install_dir: payload.installDir,
        compose_project_name: payload.composeProjectName,
        panel_url: payload.panelUrl || undefined,
        xray_api_server: payload.xrayApiServer || undefined,
        xray_api_port: payload.xrayApiPort,
        heartbeat_interval_seconds: payload.heartbeatIntervalSeconds,
        traffic_interval_seconds: payload.trafficIntervalSeconds,
        session_idle_seconds: payload.sessionIdleSeconds,
        expected_listen_ports: payload.expectedListenPorts ?? [],
        tls_cert_domains: payload.tlsCertDomains ?? [],
        disable_legacy_systemd_units: payload.disableLegacySystemdUnits ?? false,
        force_reinstall: payload.forceReinstall,
      }),
    });
    return normalizeAgentInstallGuideResult(data);
  },

  async createAccessNodeOneClickInstall(
    payload: AccessNodeOneClickInstallPayload,
  ): Promise<AccessNodeOneClickInstallResult> {
    const data = await request<unknown>(rustApiPaths.accessNodeOneClickInstall, {
      method: 'POST',
      body: JSON.stringify({
        name: payload.name,
        public_host: payload.publicHost,
        public_port: payload.publicPort,
        remark: payload.remark || undefined,
        ssh_host: payload.sshHost,
        ssh_port: payload.sshPort,
        ssh_user: payload.sshUser,
        ssh_password: payload.sshPassword || undefined,
        ssh_private_key: payload.sshPrivateKey || undefined,
        control_plane_url: payload.controlPlaneUrl || undefined,
        install_dir: payload.installDir,
        compose_project: payload.composeProject,
        ip_direct_address: payload.ipDirectAddress || undefined,
        tls_cert_domains: payload.tlsCertDomains ?? [],
        acme_email: payload.acmeEmail || undefined,
        cf_domain: payload.cfDomain || undefined,
        // CF API Token 仅安装期透传到 agent 写本机 ini，不落控制面库。
        cf_api_token: payload.cfApiToken || undefined,
        force_reinstall: payload.forceReinstall,
      }),
    });
    return normalizeAccessNodeOneClickInstallResult(data);
  },

  async getDeploymentTasks(): Promise<DeploymentTasksResult> {
    const data = await request<unknown>(rustApiPaths.deploymentTasks);
    return normalizeDeploymentTasksResult(recordValue(data).data ?? data);
  },

  // 删除/取消部署任务：后端 DELETE /api/admin/deployment-tasks/:id 删除记录，
  // 未完成任务删除即视为取消，前端列表随之移除该行。
  async deleteDeploymentTask(taskId: string) {
    await request<Record<string, unknown>>(`${rustApiPaths.deploymentTasks}/${taskId}`, {
      method: 'DELETE',
    });
  },

  async createLocalExitLines(
    accessNodeId: string,
    lines: CreateLocalExitLinePayload[],
  ): Promise<CreateLocalExitLinesResult> {
    const data = await request<unknown>(`${rustApiPaths.accessNodes}/${accessNodeId}/local-exit-lines`, {
      method: 'POST',
      body: JSON.stringify({ lines }),
    });
    return normalizeCreateLocalExitLinesResult(data);
  },

  // 列该节点已建的本机出口（self_hosted）线路；后端 GET .../local-exit-lines（Phase 4）。
  async listLocalExitLines(accessNodeId: string): Promise<LocalExitLineSummary[]> {
    const response = await request<unknown>(`${rustApiPaths.accessNodes}/${accessNodeId}/local-exit-lines`);
    const data = recordValue(response);
    return arrayValue(data.lines ?? data.local_exit_lines ?? data.items ?? response)
      .map(normalizeLocalExitLineSummary);
  },

  // 本机出口就地编辑；后端 PUT .../local-exit-lines/:endpointId（Phase 4）。
  async updateLocalExitLine(
    accessNodeId: string,
    endpointId: string,
    payload: UpdateLocalExitLinePayload,
  ) {
    await request<Record<string, unknown>>(
      `${rustApiPaths.accessNodes}/${accessNodeId}/local-exit-lines/${endpointId}`,
      { method: 'PUT', body: JSON.stringify(payload) },
    );
  },

  // 本机出口删除；后端 DELETE .../local-exit-lines/:endpointId（Phase 4）。
  async deleteLocalExitLine(accessNodeId: string, endpointId: string) {
    await request<Record<string, unknown>>(
      `${rustApiPaths.accessNodes}/${accessNodeId}/local-exit-lines/${endpointId}`,
      { method: 'DELETE' },
    );
  },

  async createExitResource(payload: {
    name: string;
    region_code?: string;
    provider_name?: string;
    ownership?: string;
    enabled?: boolean;
  }) {
    return createdId(await request<Record<string, unknown>>(rustApiPaths.exitResources, {
      method: 'POST',
      body: JSON.stringify(payload),
    }));
  },

  async updateExitResource(exitResourceId: string, payload: {
    name?: string;
    region_code?: string;
    provider_name?: string;
    ownership?: string;
    enabled?: boolean;
  }) {
    await request<Record<string, unknown>>(`${rustApiPaths.exitResources}/${exitResourceId}`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  },

  async createExitEndpoint(payload: CreateExitEndpointPayload) {
    return createdId(await request<Record<string, unknown>>(rustApiPaths.exitEndpoints, {
      method: 'POST',
      body: JSON.stringify(payload),
    }));
  },

  async updateExitEndpoint(exitEndpointId: string, payload: UpdateExitEndpointPayload) {
    await request<Record<string, unknown>>(`${rustApiPaths.exitEndpoints}/${exitEndpointId}`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  },

  async deleteExitEndpoint(exitEndpointId: string) {
    await request<Record<string, unknown>>(`${rustApiPaths.exitEndpoints}/${exitEndpointId}`, {
      method: 'DELETE',
    });
  },

  async triggerExitEndpointProbe(exitEndpointId: string) {
    await request<Record<string, unknown>>(`${rustApiPaths.exitEndpoints}/${exitEndpointId}/probe`, {
      method: 'POST',
      body: JSON.stringify({}),
    });
  },

  async getAccessLineSessions(accessLineId: string) {
    return request<Record<string, unknown>>(`${rustApiPaths.accessLines}/${accessLineId}/sessions`);
  },

  async getAccessLineMetrics(accessLineId: string) {
    return request<Record<string, unknown>>(`${rustApiPaths.accessLines}/${accessLineId}/metrics`);
  },

  async createLineGroup(payload: CreateLineGroupPayload) {
    return createdId(await request<Record<string, unknown>>(rustApiPaths.lineGroups, {
      method: 'POST',
      body: JSON.stringify(payload),
    }));
  },

  async updateLineGroup(lineGroupId: string, payload: UpdateLineGroupPayload) {
    await request<Record<string, unknown>>(`${rustApiPaths.lineGroups}/${lineGroupId}`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  },

  async deleteLineGroup(lineGroupId: string) {
    await request<Record<string, unknown>>(`${rustApiPaths.lineGroups}/${lineGroupId}`, {
      method: 'DELETE',
    });
  },

  async replaceLineGroupLines(lineGroupId: string, exitEndpointIds: string[]) {
    await request<Record<string, unknown>>(`${rustApiPaths.lineGroups}/${lineGroupId}/lines`, {
      method: 'PUT',
      body: JSON.stringify({ exit_endpoint_ids: exitEndpointIds }),
    });
  },

  async replaceLineGroupBindingNodes(lineGroupId: string, bindingNodeIds: string[]) {
    const payload: LineGroupBindingNodePayload = { binding_node_ids: bindingNodeIds };
    await request<Record<string, unknown>>(`${rustApiPaths.lineGroups}/${lineGroupId}/binding-nodes`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  },

  async listSubscriptionRuleSets(): Promise<SubscriptionRuleSet[]> {
    const response = await request<Record<string, unknown>>(rustApiPaths.subscriptionRuleSets);
    const data = recordValue(response.data ?? response);
    return arrayValue(data.rule_sets ?? data.ruleSets ?? data.items).map(normalizeSubscriptionRuleSet);
  },

  async createSubscriptionRuleSet(payload: SubscriptionRuleSetPayload) {
    return createdId(await request<Record<string, unknown>>(rustApiPaths.subscriptionRuleSets, {
      method: 'POST',
      body: JSON.stringify(payload),
    }));
  },

  async updateSubscriptionRuleSet(ruleSetId: string, payload: SubscriptionRuleSetPayload) {
    await request<Record<string, unknown>>(`${rustApiPaths.subscriptionRuleSets}/${ruleSetId}`, {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  },

  async deleteSubscriptionRuleSet(ruleSetId: string) {
    await request<Record<string, unknown>>(`${rustApiPaths.subscriptionRuleSets}/${ruleSetId}`, {
      method: 'DELETE',
    });
  },

};

function normalizeAccessNodeDeleteResult(response: Record<string, unknown>): AccessNodeDeleteResult {
  const data = recordValue(response.data ?? response);
  return {
    deletedNodeIds: arrayValue(data.deleted_node_ids ?? data.deletedNodeIds).map(stringValue),
    deletedNodeCount: Number(data.deleted_node_count ?? data.deletedNodeCount ?? 0),
    deletedAccessLineCount: Number(data.deleted_access_line_count ?? data.deletedAccessLineCount ?? 0),
    deletedLocalResourceCount: Number(data.deleted_local_resource_count ?? data.deletedLocalResourceCount ?? 0),
    deletedLocalPoolCount: Number(data.deleted_local_pool_count ?? data.deletedLocalPoolCount ?? 0),
  };
}
