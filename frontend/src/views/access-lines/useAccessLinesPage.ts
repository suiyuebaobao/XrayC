// 本文件封装中转节点页面的数据加载、表单状态和保存动作。
// 页面入口只消费这里返回的状态与事件函数，避免继续膨胀。
// 这里仅调用既有 apiClient，不修改 services/api.ts 或共享类型。
// 展示格式与纯工具函数来自同目录 format.ts，便于组件复用。
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, onMounted, onUnmounted, ref } from 'vue';
import {
  apiClient,
  type AccessEntrySummary,
  type AccessLine,
  type AccessNodeSummary,
  type DeploymentTask,
  type ExitEndpointSummary,
  type ExitPool,
  type ExitResourceSummary,
} from '@/services/api';
import { collectNodeUsedPorts } from '@/views/shared/nodeUsedPorts';
import {
  expandLinesByTransport,
  splitTextList,
  sumNullable,
} from '@/views/access-lines/format';
import { hasNewlySucceededTask } from '@/views/access-lines/deploymentTasks';
import {
  emptyInstallGuideForm,
  emptyLocalExitLinesForm,
  emptyNodeForm,
  emptyOneClickInstallForm,
} from '@/views/access-lines/forms';
import {
  buildLocalExitOutboundConfig,
  defaultStreamConfigForNetworkMode,
  normalizeLocalExitCarriage,
  normalizeLocalExitTransports,
  parseJsonObject,
} from '@/views/access-lines/localExitLines';
import type {
  InstallGuideForm,
  InstallGuideView,
  LocalExitLinesForm,
  NodeForm,
  OneClickInstallForm,
  RelayNodeView,
} from '@/views/access-lines/types';
import { useAccessNodeActions } from '@/views/access-lines/useAccessNodeActions';

export function useAccessLinesPage() {
  const loading = ref(true);
  const saving = ref(false);
  const nodes = ref<AccessNodeSummary[]>([]);
  const lines = ref<AccessLine[]>([]);
  const pools = ref<ExitPool[]>([]);
  const exitEndpoints = ref<ExitEndpointSummary[]>([]);
  // 入口列表 + 出口资源：算本机出口所属节点的「已用端口」（入口 listen_port ∪ 自建出口 endpoint port）。
  const entries = ref<AccessEntrySummary[]>([]);
  const exitResources = ref<ExitResourceSummary[]>([]);
  const deploymentTasks = ref<DeploymentTask[]>([]);
  const nodeDialogOpen = ref(false);
  const installGuideDialogOpen = ref(false);
  const oneClickInstallDialogOpen = ref(false);
  const localExitDialogOpen = ref(false);
  const localExitNode = ref<RelayNodeView | null>(null);
  const installGuide = ref<InstallGuideView | null>(null);
  const renewingTlsNodeId = ref('');
  const nodeForm = ref<NodeForm>(emptyNodeForm());
  const installGuideForm = ref<InstallGuideForm>(emptyInstallGuideForm());
  const oneClickInstallForm = ref<OneClickInstallForm>(emptyOneClickInstallForm());
  const localExitForm = ref<LocalExitLinesForm>(emptyLocalExitLinesForm());
  // 部署任务进度轮询定时器:仅在存在进行中任务时运行,所有任务到终态即停。
  let deploymentPollTimer: number | null = null;
  // 轮询间隔约 3 秒,跟项目既有轮询模式(健康检查 setInterval)对齐。
  const deploymentPollIntervalMs = 3_000;

  const relayNodes = computed<RelayNodeView[]>(() =>
    nodes.value.map((node) => {
      const nodeLines = lines.value.filter((line) => line.accessNodeId === node.id);
      const boundLineNames = Array.from(new Set(nodeLines.map(accessLineDisplayName).filter(Boolean)));
      return {
        ...node,
        lines: nodeLines,
        boundLineNames,
        onlineUsers: sumNullable(nodeLines.map((line) => line.onlineUsers ?? line.runtimeOnlineUsers)),
        activeConnections: sumNullable(nodeLines.map((line) => line.runtimeActiveConnections)),
      };
    }),
  );

  // 本机出口弹窗所属节点的已用端口集合：新建本机出口线路时据此智能填未占用端口 + 即时校验。
  const localExitNodeUsedPorts = computed(() =>
    collectNodeUsedPorts(localExitNode.value?.id ?? '', entries.value, exitEndpoints.value, exitResources.value),
  );

  const availableEndpointCount = computed(
    () => exitEndpoints.value.filter((endpoint) => endpoint.enabled && endpoint.exitResourceEnabled).length,
  );
  const totalLineCount = computed(() => lines.value.length);
  const nodeActions = useAccessNodeActions({
    saving,
    nodes,
    nodeDialogOpen,
    nodeForm,
    load,
  });

  onMounted(async () => {
    await load();
    // 初始加载后若已有进行中部署任务(刷新页面时任务仍在跑),立即启动轮询。
    ensureDeploymentPolling();
  });

  // 组件卸载时清理定时器,防止离开页面后继续轮询造成泄漏。
  onUnmounted(stopDeploymentPolling);

  async function load() {
    loading.value = true;
    try {
      const [data, endpoints, tasks, entryList, resources] = await Promise.all([
        apiClient.getControlPlane(),
        apiClient.getExitEndpoints(),
        apiClient.getDeploymentTasks(),
        apiClient.listAccessEntries(),
        apiClient.getExitResources(),
      ]);
      nodes.value = data.accessNodes;
      lines.value = data.accessLines;
      pools.value = data.exitPools;
      exitEndpoints.value = endpoints;
      deploymentTasks.value = tasks.items;
      entries.value = entryList;
      exitResources.value = resources;
    } finally {
      loading.value = false;
    }
  }

  // 是否存在进行中的部署任务:只有 waiting_for_server / running 算进行中,其余均为终态。
  function hasActiveDeploymentTask() {
    return deploymentTasks.value.some(
      (task) => task.status === 'waiting_for_server' || task.status === 'running',
    );
  }

  // 幂等启动轮询:已在轮询或当前无进行中任务时不重复起定时器。
  function ensureDeploymentPolling() {
    if (deploymentPollTimer !== null || !hasActiveDeploymentTask()) {
      return;
    }
    deploymentPollTimer = window.setInterval(() => {
      void refreshDeploymentTasks();
    }, deploymentPollIntervalMs);
  }

  // 停止轮询并复位定时器句柄,供卸载/到达终态时调用。
  function stopDeploymentPolling() {
    if (deploymentPollTimer !== null) {
      window.clearInterval(deploymentPollTimer);
      deploymentPollTimer = null;
    }
  }

  // 轮询一次只重拉部署任务列表更新进度;所有任务到终态后停止轮询。
  // 网络抖动等单次失败静默跳过,不中断定时器,保证下次仍能继续实时刷新。
  async function refreshDeploymentTasks() {
    const previous = deploymentTasks.value;
    try {
      const tasks = await apiClient.getDeploymentTasks();
      deploymentTasks.value = tasks.items;
      // 有部署任务本轮新进入成功终态:一键安装的中转节点此刻已被后端登记入库,而轮询只更新任务卡片,
      // 若不重拉读模型,新节点/入口/运行态要等用户手动刷新才出现。此处按成功跳变重拉一次,免手动刷新
      // (load 内部会同步刷新 deploymentTasks,失败则随本 catch 静默、下轮轮询再来)。
      if (hasNewlySucceededTask(previous, tasks.items)) {
        await load();
      }
    } catch {
      return;
    }
    if (!hasActiveDeploymentTask()) {
      stopDeploymentPolling();
    }
  }

  function openLocalExitLinesDialog(node: RelayNodeView) {
    localExitNode.value = node;
    // 本机出口默认 host 用节点 IP 直连地址（ip_direct_address），绝不用 publicHost（可能是 CF 域名，Bug ④）。
    localExitForm.value = emptyLocalExitLinesForm(node.id, node.ipDirectAddress);
    const labels = ['VLESS', 'Shadowsocks'];
    localExitForm.value.lines = localExitForm.value.lines.map((line, index) => ({
      ...line,
      resourceName: `${node.name} ${labels[index] ?? `线路${index + 1}`}`,
      endpointName: `${node.name} ${labels[index] ?? `线路${index + 1}`}`,
    }));
    localExitDialogOpen.value = true;
  }

  function openInstallGuideDialog() {
    resetInstallGuideForm();
    installGuideDialogOpen.value = true;
  }

  function openOneClickInstallDialog() {
    oneClickInstallForm.value = emptyOneClickInstallForm();
    oneClickInstallDialogOpen.value = true;
  }

  function retryOneClickInstallFromTask(task: DeploymentTask) {
    // SSH IP 是独立字段,重试只回填原 ssh_host;绝不退回客户连接地址(两者无关)。
    // 公网地址已从表单移除:提交时由三个直连地址自动推导,重试不再回填它。
    const sshHost = deploymentTaskMetadataText(task, 'ssh_host');
    const authMethod = deploymentTaskMetadataText(task, 'ssh_auth_method');
    oneClickInstallForm.value = {
      ...emptyOneClickInstallForm(),
      nodeName: deploymentTaskMetadataText(task, 'access_node_name') || task.title || '',
      publicPort: deploymentTaskMetadataNumber(task, 'public_port', 443),
      sshHost,
      sshPort: deploymentTaskMetadataNumber(task, 'ssh_port', 22),
      sshAuthMethod: authMethod === 'private_key' ? 'privateKey' : 'password',
      installDir: deploymentTaskMetadataText(task, 'install_dir') || '/opt/xrayc/access-agent',
      composeProject: deploymentTaskMetadataText(task, 'compose_project') || 'xrayc-access',
      forceReinstall: true,
    };
    oneClickInstallDialogOpen.value = true;
    ElMessage.info('已带入节点信息，请重新填写 SSH 凭据后重试安装');
  }

  function resetInstallGuideForm() {
    // 面板地址留空，交由后端按稳定公网中心地址 XRAYC_PUBLIC_BASE_URL 注入生成安装环境；
    // 浏览器 origin 在本机/IP/备用域名访问时远端 agent 不可达，绝不据此下发。
    installGuideForm.value = {
      ...emptyInstallGuideForm(),
      panelUrl: '',
    };
    installGuide.value = null;
  }

  async function saveLocalExitLines() {
    saving.value = true;
    try {
      const node = localExitNode.value;
      if (!node) {
        throw new Error('请选择中转服务器');
      }
      // 提交时 host 留空回退 IP 直连地址（ip_direct_address），绝不回退 publicHost（可能是 CF 域名，Bug ④）。
      const lines = expandLocalExitLinePayloads(localExitForm.value, node.ipDirectAddress);
      const result = await apiClient.createLocalExitLines(node.id, lines);
      localExitDialogOpen.value = false;
      await load();
      ElMessage.success(`已添加 ${result.createdCount} 条本机出口线路到出口管理`);
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '添加本机出口线路失败');
    } finally {
      saving.value = false;
    }
  }

  async function generateAgentInstallGuide() {
    saving.value = true;
    installGuide.value = null;
    try {
      const form = installGuideForm.value;
      const result = await apiClient.createAgentInstallGuide({
        installDir: form.installDir,
        composeProjectName: form.composeProjectName,
        panelUrl: form.panelUrl,
        xrayApiPort: form.xrayApiPort,
        expectedListenPorts: expectedInstallPorts([]),
        tlsCertDomains: splitTextList(form.tlsCertDomains),
        disableLegacySystemdUnits: form.disableLegacySystemdUnits,
        forceReinstall: form.forceReinstall,
      });
      installGuide.value = {
        summary: result.summary || '安装说明已生成',
        installCommand: result.installCommand,
        environmentText: result.environmentText,
        task: result.task,
        steps: result.steps,
      };
      await load();
      ElMessage.success('Agent 安装说明已生成');
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '生成安装说明失败');
    } finally {
      saving.value = false;
    }
  }

  async function submitOneClickInstall() {
    saving.value = true;
    try {
      const form = oneClickInstallForm.value;
      validateOneClickInstallForm(form);
      await apiClient.createAccessNodeOneClickInstall({
        name: form.nodeName.trim(),
        // 公网地址不再单独填写:由三个对外直连地址按 IP直连>域名直连>CF直连 推导
        // (优先直连 IP,避开拿 CF 域名当主机的老坑,Bug④)。
        publicHost: form.ipDirectAddress.trim() || form.certDomain.trim() || form.cfDomain.trim(),
        publicPort: Number(form.publicPort),
        remark: form.remark.trim(),
        sshHost: form.sshHost.trim(),
        sshPort: Number(form.sshPort),
        sshUser: form.sshUser.trim(),
        sshPassword: form.sshAuthMethod === 'password' ? form.sshPassword : undefined,
        sshPrivateKey: form.sshAuthMethod === 'privateKey' ? form.sshPrivateKey : undefined,
        // 控制面地址留空，交由后端按稳定公网中心地址 XRAYC_PUBLIC_BASE_URL 注入；
        // 浏览器 origin 在本机/IP/备用域名访问时远端 agent 不可达，绝不据此下发。
        controlPlaneUrl: '',
        installDir: form.installDir.trim(),
        composeProject: form.composeProject.trim(),
        ipDirectAddress: form.ipDirectAddress.trim(),
        // 域名直连地址即需 HTTP-01 签证书的灰云域名，单域名走 tls_cert_domains。
        tlsCertDomains: form.certDomain.trim() ? [form.certDomain.trim()] : [],
        acmeEmail: form.acmeEmail.trim(),
        // CF 直连地址填了即启用 CF（后端按 cf_domain 派生 cf_enabled）。
        cfDomain: form.cfDomain.trim(),
        // CF API Token 仅安装期透传给 agent 做 DNS-01，不保存。
        cfApiToken: form.cfApiToken.trim() || undefined,
        forceReinstall: true,
      });
      oneClickInstallDialogOpen.value = false;
      await load();
      // 安装任务刚创建即为进行中,启动实时进度轮询,无需用户手动刷新。
      ensureDeploymentPolling();
      ElMessage.success('安装任务已创建');
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '创建安装任务失败');
    } finally {
      saving.value = false;
    }
  }

  async function renewNodeTls(node: RelayNodeView) {
    renewingTlsNodeId.value = node.id;
    try {
      const result = await apiClient.renewAccessNodeTls(node.id);
      await load();
      ElMessage.success(result.status === 'queued' ? 'SSL 续期任务已下发' : 'SSL 续期请求已提交');
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : 'SSL 续期请求失败');
    } finally {
      renewingTlsNodeId.value = '';
    }
  }

  function expectedInstallPorts(plannedPorts: number[]) {
    return Array.from(new Set([
      ...plannedPorts,
    ]));
  }

  function validateOneClickInstallForm(form: OneClickInstallForm) {
    const requiredTextFields: Array<[string, string]> = [
      ['节点名称', form.nodeName],
      ['SSH Host', form.sshHost],
      ['SSH User', form.sshUser],
      ['安装目录', form.installDir],
      ['Compose Project', form.composeProject],
    ];
    const missingField = requiredTextFields.find(([_label, value]) => !value.trim());
    if (missingField) {
      throw new Error(`请填写${missingField[0]}`);
    }

    assertPort(Number(form.publicPort), '公网端口');
    assertPort(Number(form.sshPort), 'SSH Port');

    if (form.sshAuthMethod === 'password' && !form.sshPassword.trim()) {
      throw new Error('请填写 SSH 密码');
    }
    if (form.sshAuthMethod === 'privateKey' && !form.sshPrivateKey.trim()) {
      throw new Error('请填写 SSH 私钥');
    }
    // 至少填一个对外直连地址（IP / 域名 / CF），否则节点无法对外提供入口。
    if (!form.ipDirectAddress.trim() && !form.certDomain.trim() && !form.cfDomain.trim()) {
      throw new Error('请至少填写一个对外直连地址（IP 直连 / 域名直连 / CF 直连）');
    }
    // 填了域名直连或 CF 直连就要签证书，必须有 ACME 邮箱；CF 启用由后端按 cf_domain 派生。
    if ((form.certDomain.trim() || form.cfDomain.trim()) && !form.acmeEmail.trim()) {
      throw new Error('填写域名直连或 CF 直连地址时请填写 ACME 邮箱');
    }
  }

  function assertPort(port: number, label: string) {
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      throw new Error(`${label}必须在 1-65535 之间`);
    }
  }

  function expandLocalExitLinePayloads(form: LocalExitLinesForm, defaultHost: string) {
    const usedPorts = new Set<number>();
    return form.lines.flatMap((line, index) => {
      const basePort = Number(line.port);
      if (!Number.isInteger(basePort) || basePort < 1 || basePort > 65535) {
        throw new Error(`第 ${index + 1} 条本机出口线路端口必须在 1-65535 之间`);
      }
      const outboundConfig = buildLocalExitOutboundConfig(line, defaultHost);
      const vlessSecurity = line.vlessSecurity;
      // 传输多选 + 承载单选扇出：每个传输生成一条线路；承载在同一 RAW(tcp) 传输内合并不拆,
      // SS 选 TCP+UDP（单传输 RAW + 承载 tcp,udp）→ 仅 1 条 network_mode='tcp,udp'。
      const transports = normalizeLocalExitTransports(line.outboundType, vlessSecurity, line.transports);
      const carriage = normalizeLocalExitCarriage(line.outboundType, vlessSecurity, line.carriage);
      const expandedLines = expandLinesByTransport(transports, carriage);

      return expandedLines.map(({ networkMode }, modeIndex) => {
        // 端口按传输序号顺延(basePort+modeIndex);多传输线路会占用连续多个端口,
        // 与相邻线路/传输撞口时自动顺延到下一个空闲端口,不再报错把改端口的活儿丢给用户。
        let port = basePort + modeIndex;
        while (port <= 65535 && usedPorts.has(port)) {
          port += 1;
        }
        if (port > 65535) {
          throw new Error(`第 ${index + 1} 条本机出口线路端口分配后超过 65535，请减少传输或调小起始端口`);
        }
        usedPorts.add(port);
        return {
          resource_name: expandedLines.length > 1
            ? `${line.resourceName.trim()}-${networkMode}`.replace(/^-/, '')
            : line.resourceName.trim(),
          endpoint_name: expandedLines.length > 1
            ? `${line.endpointName.trim() || line.resourceName.trim()}-${networkMode}`.replace(/^-/, '')
            : line.endpointName.trim(),
          region_code: form.regionCode.trim(),
          outbound_type: line.outboundType,
          network_mode: networkMode,
          host: line.host.trim() || defaultHost,
          port,
          outbound_config: outboundConfig,
          stream_config: parseJsonObject(defaultStreamConfigForNetworkMode(networkMode), 'Stream 配置'),
          probe_config: {},
          // 选中的节点域名 id（免证书出口留空走 null）；后端 Phase 6 对齐同名字段。
          node_domain_id: line.nodeDomainId.trim() || null,
          enabled: line.enabled,
        };
      });
    });
  }

  function accessLineDisplayName(line: AccessLine) {
    return line.exitEndpointName || line.name || line.exitEndpointId;
  }

  function deploymentTaskMetadataText(task: DeploymentTask, key: string) {
    const value = task.safeMetadata[key];
    if (value === null || value === undefined) {
      return '';
    }
    return String(value);
  }

  function deploymentTaskMetadataNumber(task: DeploymentTask, key: string, fallback: number) {
    const value = Number(task.safeMetadata[key]);
    return Number.isFinite(value) ? value : fallback;
  }

  // 删除/取消部署任务：未结束任务确认后即取消并清理，已结束任务即删除列表残留。
  async function deleteDeploymentTask(task: DeploymentTask) {
    const unfinished = task.status === 'waiting_for_server' || task.status === 'running';
    const actionLabel = unfinished ? '取消' : '删除';
    try {
      await ElMessageBox.confirm(
        `确认${actionLabel}该部署任务？${unfinished ? '未完成的安装将被中止。' : '仅清理控制台任务记录，不影响已安装节点。'}`,
        `${actionLabel}部署任务`,
        { type: 'warning', confirmButtonText: actionLabel, cancelButtonText: '返回' },
      );
    } catch {
      return;
    }
    try {
      await apiClient.deleteDeploymentTask(task.id);
      await load();
      ElMessage.success(`部署任务已${actionLabel}`);
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : `${actionLabel}部署任务失败`);
    }
  }

  return {
    loading,
    saving,
    nodes,
    lines,
    pools,
    exitEndpoints,
    deploymentTasks,
    nodeDialogOpen,
    installGuideDialogOpen,
    oneClickInstallDialogOpen,
    localExitDialogOpen,
    installGuide,
    localExitNode,
    nodeForm,
    installGuideForm,
    oneClickInstallForm,
    localExitForm,
    renewingTlsNodeId,
    relayNodes,
    localExitNodeUsedPorts,
    availableEndpointCount,
    totalLineCount,
    load,
    ...nodeActions,
    openLocalExitLinesDialog,
    openInstallGuideDialog,
    openOneClickInstallDialog,
    saveLocalExitLines,
    generateAgentInstallGuide,
    submitOneClickInstall,
    retryOneClickInstallFromTask,
    deleteDeploymentTask,
    renewNodeTls,
  };
}
