<!--
  本弹窗用于批量创建中转服务器自建出口线路。
  保存后只写入出口管理，不自动创建订阅入口，也不会加入分组。
  每一行先选「连接方式」（IP 直连/域名直连），由连接方式驱动域名选择 + 协议选项；本机出口是同机自家出口，走 CF 无意义，故不提供 CF 直连。
-->
<script setup lang="ts">
import { ElMessage } from 'element-plus';
import { computed, watch } from 'vue';
import { nextFreePort, portConflictMessage } from '@/views/shared/nextFreePort';
import LocalExitLineRow from '@/views/access-lines/LocalExitLineRow.vue';
import LocalExitLinesList from '@/views/access-lines/LocalExitLinesList.vue';
import { emptyLocalExitLineForm } from '@/views/access-lines/forms';
import {
  defaultLocalExitLineDetails,
  defaultServerName,
  defaultTlsCertificateFile,
  defaultTlsKeyFile,
  localExitCarriageOptionsFor,
  localExitConnectionModeLabels,
  localExitConnectionModeToDomainKind,
  localExitProtocolOptionsForDomainKind,
  localExitTransportOptionsFor,
  localExitVlessSecurityOptionsForDomainKind,
  normalizeLocalExitCarriage,
  normalizeLocalExitTransports,
  type LocalExitConnectionMode,
  type LocalExitDomainKind,
  type LocalExitLineFormWithMode,
} from '@/views/access-lines/localExitLines';
import type { LocalExitLinesForm, RelayNodeView } from '@/views/access-lines/types';

const visible = defineModel<boolean>({ required: true });
const form = defineModel<LocalExitLinesForm>('form', { required: true });

const props = defineProps<{
  node: RelayNodeView | null;
  // 该节点已用端口（启用入口 listen_port ∪ 已建本机出口 endpoint port）：新建线路智能默认端口据此避开 + 即时校验。
  usedPorts: Set<number>;
  saving: boolean;
}>();

const emit = defineEmits<{
  (event: 'save'): void;
  // 已建线路就地编辑/删除成功后向页面冒泡，触发控制面刷新。
  (event: 'changed'): void;
}>();

// 本机出口 host 默认/回退地址：取节点 IP 直连地址（ip_direct_address），绝不回退 publicHost。
// publicHost 是客户连接地址，可能是 CF（橙云）域名；本机出口是同机自家出口、中转直接连它，
// 走 CF 会经边缘连不到出口端口 → 探测 offline → 订阅"无可用线路"（Bug ④）。无 IP 直连地址时
// 留空，由后端回退本机回环地址 127.0.0.1，仍不碰 CF。
function currentNodeHost() {
  return props.node?.ipDirectAddress.trim() || '';
}

// 本机出口服务地址跟随连接方式：IP 直连用节点 IP（ip_direct_address），域名直连用选中的直连域名。
function localExitHostForMode(line: LocalExitLineFormWithMode) {
  if (lineConnectionMode(line) === 'direct') {
    const selected = nodeDomainOptions.value.find((item) => item.id === line.nodeDomainId);
    const domain = selected?.domain?.trim();
    if (domain) {
      return domain;
    }
  }
  return currentNodeHost();
}

// 选中节点的域名清单（直连 + CF）；连接方式选 direct/cf 后据此挑一个该 kind 的域名（IP 直连留空=免证书）。
const nodeDomainOptions = computed(() => props.node?.domains ?? []);

// 本机出口的「实际出网 IP」= 所属节点公网 IP（优先 ip_direct_address，无则 public_host）。
// 仅作参考展示给管理员，让其明白本机出口虽用 127.0.0.1 回环地址连接服务，但流量从节点公网 IP 出网。
const nodeExitIp = computed(() => {
  const direct = props.node?.ipDirectAddress?.trim();
  if (direct) {
    return direct;
  }
  return props.node?.publicHost?.trim() ?? '';
});

// 节点可用的「连接方式」：IP 直连永远有；有 direct 域名才出现"域名直连"。本机出口是同机自家出口，
// 走 CF 无意义，故即便节点配了 CF 域名也不提供 CF 直连。纯 IP 节点只剩 ip，要证书协议根本不出现在协议下拉里。
const availableConnectionModes = computed<LocalExitConnectionMode[]>(() => {
  const modes: LocalExitConnectionMode[] = ['ip'];
  const domains = nodeDomainOptions.value;
  if (domains.some((item) => item.kind === 'direct')) {
    modes.push('direct');
  }
  return modes;
});

// 连接方式下拉选项（label + value），只列该节点可用的连接方式。
const connectionModeOptions = computed(() =>
  availableConnectionModes.value.map((mode) => ({
    value: mode,
    label: localExitConnectionModeLabels[mode],
  })),
);

// 把表单行视作带 connectionMode 的运行期类型（字段由组件初始化，提交仍只看 nodeDomainId）。
const lines = computed(() => form.value.lines as LocalExitLineFormWithMode[]);

// 取某条线路的连接方式，未初始化或已不可用（如切到纯 IP 节点）时回退到 ip。
function lineConnectionMode(line: LocalExitLineFormWithMode): LocalExitConnectionMode {
  const mode = line.connectionMode;
  if (mode && availableConnectionModes.value.includes(mode)) {
    return mode;
  }
  return 'ip';
}

// 取某条线路的域名 kind：由连接方式推导（ip→''、direct→'direct'）；本机出口无 cf 连接方式，故不会产生 'cf'。
function lineDomainKind(line: LocalExitLineFormWithMode): LocalExitDomainKind {
  return localExitConnectionModeToDomainKind(lineConnectionMode(line));
}

// 某条线路在当前连接方式（direct）下可选的域名列表（按该 kind 过滤；ip 不显示二级下拉）。
function domainOptionsFor(line: LocalExitLineFormWithMode) {
  const kind = lineDomainKind(line);
  if (kind !== 'direct') {
    return [];
  }
  return nodeDomainOptions.value.filter((item) => item.kind === kind);
}

// 该 kind 下默认选中的域名：优先主域名（isPrimary），否则第一个。
function defaultDomainIdFor(line: LocalExitLineFormWithMode): string {
  const options = domainOptionsFor(line);
  if (options.length === 0) {
    return '';
  }
  const primary = options.find((item) => item.isPrimary);
  return (primary ?? options[0]).id;
}

// 按某条线路连接方式（推导出的域名 kind）过滤协议/安全模式选项（与后端按选中域名 kind 判定护栏同口径）。
function protocolOptionsFor(line: LocalExitLineFormWithMode) {
  return localExitProtocolOptionsForDomainKind(lineDomainKind(line));
}

function vlessSecurityOptionsFor(line: LocalExitLineFormWithMode) {
  return localExitVlessSecurityOptionsForDomainKind(lineDomainKind(line));
}

// 传输多选选项（每个传输 = 一条线路）；承载单选选项（RAW 传输的 L4，SS/SOCKS 可合并 tcp,udp）。
function transportOptionsFor(line: LocalExitLineFormWithMode) {
  return localExitTransportOptionsFor(line.outboundType, line.vlessSecurity);
}

function carriageOptionsFor(line: LocalExitLineFormWithMode) {
  return localExitCarriageOptionsFor(line.outboundType, line.vlessSecurity);
}

// 仅当选中 RAW(tcp) 传输时才需要承载下拉；HY2（固定 udp）无 RAW 承载。
function needsCarriageFor(line: LocalExitLineFormWithMode) {
  return line.transports.includes('tcp');
}

// 把当前行的传输/承载收敛到当前协议×安全允许项，避免提交被后端拒绝。
function normalizeLineNetwork(line: LocalExitLineFormWithMode) {
  line.transports = normalizeLocalExitTransports(line.outboundType, line.vlessSecurity, line.transports);
  line.carriage = normalizeLocalExitCarriage(line.outboundType, line.vlessSecurity, line.carriage);
}

// 按某条线路连接方式（域名 kind）把不允许的协议/安全模式回退到允许项，再收敛传输/承载。
function coerceLineToAllowedProtocol(line: LocalExitLineFormWithMode) {
  const allowedProtocols = new Set(protocolOptionsFor(line).map((option) => option.value));
  if (!allowedProtocols.has(line.outboundType)) {
    // direct/ip 均含 vless；统一回退到 vless 再按域名 kind 归一安全模式与传输/承载。
    line.outboundType = allowedProtocols.has('vless') ? 'vless' : 'socks';
    protocolChanged(line);
  }
  const allowedSecurities = new Set(vlessSecurityOptionsFor(line).map((option) => option.value));
  if (line.outboundType === 'vless' && !allowedSecurities.has(line.vlessSecurity)) {
    line.vlessSecurity = allowedSecurities.has('reality') ? 'reality' : 'tls';
    vlessSecurityChanged(line);
  }
  normalizeLineNetwork(line);
}

// 按连接方式同步该行选中域名：ip 清空 nodeDomainId；direct/cf 时若当前选中域名不属于该 kind，则回落到该 kind 主域名/第一个。
function syncDomainSelectionForMode(line: LocalExitLineFormWithMode) {
  if (lineConnectionMode(line) === 'ip') {
    line.nodeDomainId = '';
    return;
  }
  const options = domainOptionsFor(line);
  const stillValid = options.some((item) => item.id === line.nodeDomainId);
  if (!stillValid) {
    line.nodeDomainId = defaultDomainIdFor(line);
  }
}

// 按当前节点把每条线路初始化到合法状态：收敛连接方式 → 同步 nodeDomainId → 收敛协议/安全/网络。
function initLinesForNode() {
  lines.value.forEach((line) => {
    line.connectionMode = lineConnectionMode(line);
    syncDomainSelectionForMode(line);
    coerceLineToAllowedProtocol(line);
  });
}

// 切换某条线路的连接方式：先同步选中域名，再按新 kind 收敛协议/安全模式/网络模式。
function connectionModeChanged(line: LocalExitLineFormWithMode) {
  syncDomainSelectionForMode(line);
  coerceLineToAllowedProtocol(line);
  // 地址跟随连接方式带出：IP 直连→节点 IP、域名直连→所选域名（可手改）。
  line.host = localExitHostForMode(line);
  syncServerNameFromHost(line);
}

// 切换某条线路选中的节点域名：同 kind 内换域名一般不变 kind，仍按当前 kind 收敛兜底。
function nodeDomainChanged(line: LocalExitLineFormWithMode) {
  coerceLineToAllowedProtocol(line);
  // 换域名后地址同步带出新域名。
  line.host = localExitHostForMode(line);
  syncServerNameFromHost(line);
}

// 弹窗可见或切换节点时，按当前连接方式初始化各行：纯 IP 节点只可选 ip + 免证书协议。
watch(
  () => [visible.value, props.node?.id ?? ''] as const,
  ([isVisible]) => {
    if (isVisible) {
      initLinesForNode();
      // 收敛协议/传输后再按节点已用端口分配默认端口，避开入口 / 已建本机出口占用的端口。
      assignDefaultPorts();
    }
  },
  { immediate: true },
);

function syncDefaultHostToLines(nextHost = currentNodeHost(), previousHost = '') {
  const normalizedNextHost = nextHost.trim();
  if (!normalizedNextHost) {
    return;
  }
  const oldDefault = previousHost.trim();
  lines.value.forEach((line) => {
    const currentHost = line.host.trim();
    if (!currentHost || (oldDefault && currentHost === oldDefault)) {
      line.host = normalizedNextHost;
      syncServerNameFromHost(line);
    }
  });
}

watch(
  // 默认 host 跟随节点 IP 直连地址（ip_direct_address），不再跟 publicHost（可能是 CF 域名，Bug ④）。
  () => [visible.value, props.node?.id ?? '', props.node?.ipDirectAddress ?? '', form.value.lines.length] as const,
  (nextValue, previousValue) => {
    const [isVisible, _nodeId, nextHost] = nextValue;
    const previousHost = previousValue?.[2] ?? '';
    if (isVisible) {
      syncDefaultHostToLines(nextHost, previousHost);
    }
  },
  { immediate: true },
);

function addLine() {
  form.value.lines.push(emptyLocalExitLineForm('vless', currentNodeHost(), nextPort()));
  // 新行默认 IP 直连（免证书）：刚 push 的行还没 connectionMode，初始化时回退为 ip 并收敛协议。
  initLinesForNode();
}

function removeLine(index: number) {
  form.value.lines.splice(index, 1);
  if (form.value.lines.length === 0) {
    addLine();
  }
}

// 汇总「已占用端口」：节点已用端口（入口 ∪ 已建本机出口）∪ 当前草稿各行已占端口（多传输行占连续端口）。
// excludeIndex 排除某一行自身，用于给该行算默认端口/校验时不把自己算成占用。
function draftUsedPorts(excludeIndex = -1): Set<number> {
  const used = new Set<number>(props.usedPorts);
  form.value.lines.forEach((line, index) => {
    if (index === excludeIndex) {
      return;
    }
    const base = Number(line.port);
    if (!Number.isInteger(base) || base <= 0) {
      return;
    }
    const span = Math.max(1, line.transports?.length ?? 1);
    for (let offset = 0; offset < span; offset += 1) {
      used.add(base + offset);
    }
  });
  return used;
}

// 新增线路的默认端口：在「节点已用 ∪ 现有草稿行」之外挑首个未占用的防封友好端口（本机出口口径）。
function nextPort() {
  return nextFreePort('local-exit', draftUsedPorts()) ?? 1443;
}

// 弹窗打开时给预填的各行分配未占用的默认端口，避免默认 1443/1444 撞上入口或已建本机出口。
function assignDefaultPorts() {
  form.value.lines.forEach((_line, index) => {
    const port = nextFreePort('local-exit', draftUsedPorts(index));
    if (port !== null) {
      form.value.lines[index].port = port;
    }
  });
}

// 某行端口即时校验：手填端口撞上节点已用端口（入口 / 已建本机出口）时红字提示；草稿行间冲突提交时会自动顺延，不在此报。
function linePortError(line: LocalExitLineFormWithMode) {
  return portConflictMessage('local-exit', Number(line.port), props.usedPorts);
}

// 提交前拦截：任一行端口撞节点已用端口就提示并中止，别把 OS 端口绑定冲突留到运行期。
function handleSave() {
  const conflicted = lines.value.some((line) => linePortError(line) !== '');
  if (conflicted) {
    ElMessage.warning('有本机出口线路的端口已被占用，请换一个再提交');
    return;
  }
  emit('save');
}

function protocolChanged(line: LocalExitLineFormWithMode) {
  if (!line.host.trim()) {
    line.host = currentNodeHost();
  }
  Object.assign(line, defaultLocalExitLineDetails(line.outboundType, line.host));
  normalizeLineNetwork(line);
}

function networkChanged(line: LocalExitLineFormWithMode) {
  normalizeLineNetwork(line);
}

function vlessSecurityChanged(line: LocalExitLineFormWithMode) {
  if (line.vlessSecurity !== 'none' && !line.serverName.trim()) {
    syncServerNameFromHost(line);
  }
  normalizeLineNetwork(line);
}

function hostChanged(line: LocalExitLineFormWithMode) {
  syncServerNameFromHost(line);
}

function serverNameChanged(line: LocalExitLineFormWithMode) {
  syncTlsFilesFromServerName(line);
}

function syncServerNameFromHost(line: LocalExitLineFormWithMode) {
  const serverName = defaultServerName(line.outboundType, line.host || currentNodeHost(), line.vlessSecurity);
  if (serverName) {
    line.serverName = serverName;
  }
  syncTlsFilesFromServerName(line);
}

function syncTlsFilesFromServerName(line: LocalExitLineFormWithMode) {
  const serverName = line.serverName.trim();
  if (!serverName) {
    return;
  }
  if (shouldUseDefaultTlsPath(line.tlsCertificateFile)) {
    line.tlsCertificateFile = defaultTlsCertificateFile(serverName);
  }
  if (shouldUseDefaultTlsPath(line.tlsKeyFile)) {
    line.tlsKeyFile = defaultTlsKeyFile(serverName);
  }
}

function shouldUseDefaultTlsPath(value: string) {
  const normalized = value.trim();
  return !normalized || normalized.startsWith('/etc/letsencrypt/live/');
}
</script>

<template>
  <el-dialog v-model="visible" title="本机出口服务" width="1080px">
    <el-alert
      class="dialog-tip"
      type="info"
      show-icon
      :closable="false"
      title="这里仅批量添加本机创建的出口线路到出口管理，不会创建订阅节点，也不会自动绑定中转节点。"
    />
    <el-form label-position="top">
      <el-row :gutter="16">
        <el-col :span="12">
          <el-form-item label="所属中转服务器">
            <el-input :model-value="node ? `${node.name} / ${node.publicHost}` : '-'" disabled />
            <div v-if="node" class="node-exit-hint">
              本机出口实际出网 IP（节点公网）：
              <span v-if="nodeExitIp" class="node-exit-hint__ip">{{ nodeExitIp }}</span>
              <span v-else>未配置（请先在节点补全 ip_direct_address / public_host）</span>
            </div>
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="地区代码">
            <el-input v-model="form.regionCode" placeholder="例如 US、JP、SG；可留空" />
          </el-form-item>
        </el-col>
      </el-row>

      <LocalExitLinesList :node="node" :active="visible" @changed="emit('changed')" />

      <el-divider content-position="left">新增本机出口线路</el-divider>

      <div class="line-list">
        <LocalExitLineRow
          v-for="(line, index) in lines"
          :key="index"
          :line="line"
          :index="index"
          :node-exit-ip="nodeExitIp"
          :connection-mode-options="connectionModeOptions"
          :domain-options="domainOptionsFor(line)"
          :protocol-options="protocolOptionsFor(line)"
          :vless-security-options="vlessSecurityOptionsFor(line)"
          :transport-options="transportOptionsFor(line)"
          :carriage-options="carriageOptionsFor(line)"
          :needs-carriage="needsCarriageFor(line)"
          :port-error="linePortError(line)"
          @remove="removeLine(index)"
          @connection-mode-change="connectionModeChanged(line)"
          @node-domain-change="nodeDomainChanged(line)"
          @protocol-change="protocolChanged(line)"
          @network-change="networkChanged(line)"
          @vless-security-change="vlessSecurityChanged(line)"
          @host-change="hostChanged(line)"
          @server-name-change="serverNameChanged(line)"
        />
      </div>
      <el-button type="primary" plain @click="addLine">添加线路</el-button>
    </el-form>

    <template #footer>
      <el-button @click="visible = false">取消</el-button>
      <el-button type="primary" :loading="saving" @click="handleSave">添加到出口管理</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.dialog-tip {
  margin-bottom: 16px;
}

.line-list {
  display: flex;
  flex-direction: column;
  gap: 16px;
  margin-bottom: 16px;
}

.node-exit-hint {
  margin-top: 4px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--el-text-color-secondary);
}

.node-exit-hint__ip {
  color: var(--el-text-color-primary);
  font-weight: 600;
}
</style>
