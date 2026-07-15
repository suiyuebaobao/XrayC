<script setup lang="ts">
import { computed, watch } from 'vue';
import type { AccessNodeSummary } from '@/services/api';
import EntryCdnFields from '@/views/access-entries/EntryCdnFields.vue';
import EntryListenFields from '@/views/access-entries/EntryListenFields.vue';
import EntryProtocolFields from '@/views/access-entries/EntryProtocolFields.vue';
import {
  CF_SUPPORTED_HTTPS_PORTS,
  isCfSupportedHttpsPort,
  protocolsForAddressType,
  type EntryAddressType,
  type EntryProtocol as AddressEntryProtocol,
} from '@/views/access-entries/entryProtocolMatrix';
import {
  ADDRESS_TYPE_HINTS,
  availableConnectionModes as deriveAvailableModes,
  connectionModeOptions as deriveModeOptions,
  deriveInitialConnectionMode,
  firstNodeDomainId,
} from '@/views/access-entries/entryConnectionMode';
import {
  carriageOptionsFor,
  normalizeCarriageValue,
  protocolOptions,
  transportOptionsForProtocolSecurity,
  type VlessEntrySecurityMode,
} from '@/views/access-lines/format';
import type { AccessEntryFormState } from '@/views/access-entries/entryFormState';

const props = defineProps<{
  accessNodes: AccessNodeSummary[];
  entryForm: AccessEntryFormState;
  modelValue: boolean;
  saving: boolean;
  title: string;
  // 端口即时校验文案（页面据节点已用端口/CF 端口合规算出）：非空时在端口字段下红字提示。
  portError: string;
}>();

const emit = defineEmits<{
  (event: 'access-node-change', accessNodeId: string): void;
  // 连接方式变化（IP/域名/CF 切换）：页面据此按新连接方式重算智能默认端口。
  (event: 'connection-mode-change'): void;
  // 管理员手动改了直连地址 / CF 域名，页面据此标脏不再自动覆盖。
  (event: 'listen-host-input'): void;
  (event: 'cdn-hostname-input'): void;
  // CDN 开关变化（直连↔CF），页面据此把地址默认值在直连地址与 CF 域名间切换。
  (event: 'cdn-mode-change'): void;
  // 选中的节点域名变化：页面据此把监听地址带出该域名，并按 kind 重置护栏。
  (event: 'node-domain-change', nodeDomainId: string): void;
  // 协议/安全/网络归一完成：页面据此重派生 Server Name / 路径 / Host 默认值。
  (event: 'fields-normalized'): void;
  // 管理员手改 Server Name / 路径 / Host，页面据此标脏不再自动覆盖。
  (event: 'server-name-input'): void;
  (event: 'transport-path-input'): void;
  (event: 'transport-host-input'): void;
  (event: 'save'): void;
  (event: 'update:modelValue', value: boolean): void;
}>();

type EntryProtocol = AddressEntryProtocol;

// 选中节点的域名清单（直连 + CF）；下拉据此列出可选域名，留空=免证书。
const nodeDomainOptions = computed(() => selectedNode.value?.domains ?? []);

// 当前选中的节点域名（按 id 命中），用于带出监听地址与连接方式反推。
const selectedNodeDomain = computed(
  () => nodeDomainOptions.value.find((domain) => domain.id === props.entryForm.nodeDomainId) ?? null,
);

// 当前入口地址类型：直接取用户选的连接方式（不再走启发式推导），显式驱动协议/CDN/选域名护栏。
const addressType = computed<EntryAddressType>(() => props.entryForm.connectionMode);
// 当前节点支持的连接方式（恒含 ip，按域名 kind 加 domain/cf）；下拉项同口径。纯派生见 entryConnectionMode。
const availableConnectionModes = computed<EntryAddressType[]>(() => deriveAvailableModes(nodeDomainOptions.value));
const connectionModeOptions = computed(() => deriveModeOptions(nodeDomainOptions.value));

// 「选用域名」下拉按当前连接方式过滤：domain→direct 域名；cf→cf 域名；ip 模式下拉本就不展示。
const connectionModeDomainOptions = computed(() => {
  const kind = props.entryForm.connectionMode === 'cf' ? 'cf' : 'direct';
  return nodeDomainOptions.value.filter((domain) => domain.kind === kind);
});
// 该地址类型下允许的协议集合（与后端护栏对齐）。
const allowedProtocols = computed<EntryProtocol[]>(() => protocolsForAddressType(addressType.value));
// 协议下拉只展示当前地址类型可用项：IP→Reality/SS；域名→全族；CF→VLESS/Trojan。
const availableProtocolOptions = computed(() =>
  protocolOptions.filter((option) => allowedProtocols.value.includes(option.value as EntryProtocol)),
);
// CF（橙云）模式：协议限 VLESS/Trojan、传输限 WS/gRPC/XHTTP、安全限 TLS。
const isCfAddress = computed(() => addressType.value === 'cf');

// 选中的接入节点；用于在节点启用 CF 时给入口表单提示。
const selectedNode = computed(() =>
  props.accessNodes.find((node) => node.id === props.entryForm.accessNodeId) ?? null,
);

const securityOptions = computed(() => {
  if (props.entryForm.protocol === 'vless') {
    // IP 直连无证书：VLESS 可选 Reality（抗封伪装）或普通（裸连免证书，UDP/XUDP/WS 全开）；TLS 需证书故不给。CF 仅 TLS；域名直连只 TLS。
    if (addressType.value === 'ip') {
      return [
        { label: 'Reality', value: 'reality' },
        { label: '普通 VLESS', value: '' },
      ];
    }
    if (isCfAddress.value) {
      return [{ label: 'TLS', value: 'tls' }];
    }
    // 域名直连：只 TLS（用证书）；Reality 属 IP 直连、普通 none 不用证书，均不在此。
    return [{ label: 'TLS', value: 'tls' }];
  }
  if (props.entryForm.protocol === 'trojan' || props.entryForm.protocol === 'hysteria') {
    return [{ label: 'TLS', value: 'tls' }];
  }
  return [{ label: 'None', value: '' }];
});

// 传输选项（streamSettings.network，可多选，每个传输 = 一条入口）。CF（橙云）只放行 WS/gRPC/XHTTP。
const transportOptions = computed(() => {
  const options = transportOptionsForProtocolSecurity(
    props.entryForm.protocol,
    vlessNetworkSecurity(props.entryForm.security),
  );
  if (isCfAddress.value) {
    return options.filter((option) => ['ws', 'grpc', 'xhttp'].includes(option.value));
  }
  return options;
});
// 承载选项（L4 单选合并，仅 RAW(tcp) 传输生效）。needsCarriage 仅在选中 RAW 时展示承载下拉。
const carriageOptions = computed(() =>
  carriageOptionsFor(props.entryForm.protocol, vlessNetworkSecurity(props.entryForm.security)),
);
const selectedTransports = computed(() => currentTransports());
const needsCarriage = computed(() => selectedTransports.value.includes('tcp'));
const needsTransportPath = computed(() =>
  selectedTransports.value.some((mode) => ['ws', 'xhttp', 'grpc'].includes(mode)),
);
const needsTransportHost = computed(() =>
  selectedTransports.value.some((mode) => ['ws', 'xhttp'].includes(mode)),
);
const transportPathLabel = computed(() =>
  selectedTransports.value.length === 1 && selectedTransports.value[0] === 'grpc' ? 'gRPC 服务名' : '路径',
);
const transportPathPlaceholder = computed(() =>
  selectedTransports.value.length === 1 && selectedTransports.value[0] === 'grpc' ? 'xrayc' : '/xrayc',
);

// 协议下拉的地址类型提示文案（按当前连接方式取，文案表见 entryConnectionMode）。
const addressTypeHint = computed(() => ADDRESS_TYPE_HINTS[addressType.value]);

watch(
  () => props.modelValue,
  (open) => {
    if (open) {
      // 先按当前表单反推连接方式（编辑老入口能正确回填），再归一协议/安全/传输/端口。
      initConnectionMode();
      normalizeEntryProtocolFields();
    }
  },
);

// 地址类型变化（切连接方式 / 切节点收敛）时，重新归一协议、安全模式和传输，保证只剩可用项。
watch(addressType, () => {
  if (props.modelValue) {
    normalizeEntryProtocolFields();
  }
});

// 切节点后旧连接方式可能不再可用（新节点无对应 kind 域名）：收敛到可用集，不可用则回退 IP。
// 此处只处理「连接方式失效」一种情况；地址/域名清空与 CDN 关闭交给页面 access-node-change 流程。
watch(selectedNode, () => {
  if (!props.modelValue) {
    return;
  }
  const mode = props.entryForm.connectionMode;
  // 连接方式在新节点不可用 → 回退 IP（清域名/关 CDN/归一协议）。
  if (!availableConnectionModes.value.includes(mode)) {
    handleConnectionModeChange('ip');
    return;
  }
  // 连接方式仍可用，但选中域名属于旧节点（页面已清空）→ 重选新节点对应 kind 的首个域名。
  if (mode !== 'ip' && !connectionModeDomainOptions.value.some((domain) => domain.id === props.entryForm.nodeDomainId)) {
    handleConnectionModeChange(mode);
  }
});

// 打开弹窗时按当前表单反推连接方式（cdnEnabled→cf；否则按选中域名 kind；不可用回退 ip），编辑老入口能正确回填。
function initConnectionMode() {
  props.entryForm.connectionMode = deriveInitialConnectionMode(
    nodeDomainOptions.value,
    props.entryForm.cdnEnabled,
    selectedNodeDomain.value?.kind,
  );
}

// 设置 CDN 开关并在变更时通知页面在直连地址/CF 域名间重带地址默认值。
function setCdnEnabled(enabled: boolean) {
  if (props.entryForm.cdnEnabled !== enabled) {
    props.entryForm.cdnEnabled = enabled;
    emit('cdn-mode-change');
  }
}

// 连接方式变化：显式驱动选域名 + CDN，再归一协议/安全/网络/端口。
// ip→清域名、关 CDN；domain→关 CDN、选第一个 direct 域名（优先主域名）；cf→开 CDN、选第一个 cf 域名。
function handleConnectionModeChange(mode: EntryAddressType) {
  props.entryForm.connectionMode = mode;
  if (mode === 'ip') {
    setCdnEnabled(false);
    props.entryForm.nodeDomainId = '';
    // IP 直连无选域名：通知页面按 IP 模式重带监听地址（节点 IP，未手改才覆盖）。
    emit('node-domain-change', '');
  } else {
    setCdnEnabled(mode === 'cf');
    // CF 直连自动把 CDN 服务商填成 Cloudflare（cdnEnabled 已由 setCdnEnabled 置真）。
    if (mode === 'cf') {
      props.entryForm.cdnProvider = 'cloudflare';
    }
    props.entryForm.nodeDomainId = firstNodeDomainId(nodeDomainOptions.value, mode === 'cf' ? 'cf' : 'direct');
    emit('node-domain-change', props.entryForm.nodeDomainId);
  }
  normalizeEntryProtocolFields();
  // 连接方式变了：通知页面按新连接方式（CF/非 CF）重算智能默认端口（在 normalize 的 CF 端口回落之后覆盖）。
  emit('connection-mode-change');
}

function handleProtocolChange() {
  normalizeEntryProtocolFields();
}

// 选中节点域名变化：通知页面带出监听地址；addressType watcher 会随之重置协议护栏。
function handleNodeDomainChange(nodeDomainId: string) {
  emit('node-domain-change', nodeDomainId);
}

// 节点域名下拉的标签：域名 + 类型徽标（直连/CF）+ 主域名标记，便于区分。
function nodeDomainOptionLabel(domain: { domain: string; kind: 'direct' | 'cf'; isPrimary: boolean }) {
  const kindLabel = domain.kind === 'cf' ? 'CF' : '直连';
  return `${domain.domain}（${kindLabel}${domain.isPrimary ? ' · 主' : ''}）`;
}

function handleSecurityChange() {
  normalizeEntryProtocolFields();
}

function normalizeEntryProtocolFields() {
  // 地址类型变化（切节点/切直连↔CF）后，把不在当前类型可用集里的协议回退到第一个可用项。
  if (!allowedProtocols.value.includes(props.entryForm.protocol as EntryProtocol)) {
    props.entryForm.protocol = allowedProtocols.value[0] ?? 'vless';
  }

  // CF（橙云）入口：监听端口若不在 CF 支持的 HTTPS 端口集合内，回落到 443，避免到 CF 边缘连不上。
  if (isCfAddress.value && !isCfSupportedHttpsPort(Number(props.entryForm.listenPort))) {
    props.entryForm.listenPort = CF_SUPPORTED_HTTPS_PORTS[0];
  }

  if (props.entryForm.protocol === 'vless') {
    if (addressType.value === 'ip') {
      // IP 直连无证书：VLESS 只能 Reality 或普通（none）；TLS 需证书故排除。显式选了普通就保留，其余一律回落 Reality。
      props.entryForm.security = props.entryForm.security === '' ? '' : 'reality';
    } else if (isCfAddress.value) {
      // CF 走 TLS（CF↔节点回源），不允许 Reality/裸 VLESS。
      props.entryForm.security = 'tls';
    } else {
      // 域名直连：VLESS 只 TLS（去 Reality/普通），与 securityOptions 同口径。
      props.entryForm.security = 'tls';
    }
  } else if (props.entryForm.protocol === 'trojan' || props.entryForm.protocol === 'hysteria') {
    props.entryForm.security = 'tls';
  } else {
    props.entryForm.security = '';
  }

  const security = vlessNetworkSecurity(props.entryForm.security);

  if (props.entryForm.protocol === 'hysteria') {
    // HY2 固定单传输 + UDP 承载，无需选择。
    setTransports(['hysteria']);
    props.entryForm.carriage = 'udp';
    emit('fields-normalized');
    return;
  }

  // 归一传输：去掉不在当前协议×安全允许集里的项；CF（橙云）只放行 WS/gRPC/XHTTP。
  const transportValues = transportOptionsForProtocolSecurity(props.entryForm.protocol, security).map(
    (option) => option.value,
  );
  const allowedTransports = new Set(
    isCfAddress.value ? transportValues.filter((value) => ['ws', 'grpc', 'xhttp'].includes(value)) : transportValues,
  );
  let transports = currentTransports().filter((transport) => allowedTransports.has(transport));
  if (transports.length === 0) {
    transports = [allowedTransports.values().next().value ?? (isCfAddress.value ? 'ws' : 'tcp')];
  }
  setTransports(transports);
  // 归一承载到当前协议×安全允许的合并项（合并值如 tcp,udp；提交时再降维成入口单值模式）。
  props.entryForm.carriage = normalizeCarriageValue(props.entryForm.protocol, security, props.entryForm.carriage);

  // 归一后通知页面：由页面统一据当前协议×安全×传输重派生未手改的 Server Name / 路径 / Host 默认值。
  emit('fields-normalized');
}

function currentTransports() {
  const transports = props.entryForm.transports.length > 0
    ? props.entryForm.transports
    : [props.entryForm.transport];
  return transports.filter((transport, index, values) => transport && values.indexOf(transport) === index);
}

function setTransports(transports: string[]) {
  props.entryForm.transports.splice(0, props.entryForm.transports.length, ...transports);
  props.entryForm.transport = transports[0] ?? 'tcp';
}

function vlessNetworkSecurity(value: string): VlessEntrySecurityMode {
  const normalized = props.entryForm.protocol === 'vless' ? value : 'none';
  return normalized === 'tls' || normalized === 'reality' ? normalized : 'none';
}

// 选项 label 用于折叠态/选中态展示与 filterable 搜索；CF 节点追加「 (CF)」标识便于区分。
function nodeOptionLabel(node: AccessNodeSummary) {
  const base = node.name || node.publicHost || node.id;
  return node.cfEnabled ? `${base} (CF)` : base;
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    :title="title"
    width="760px"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <el-form label-position="top">
      <!-- ① 基础信息：入口名称 / 启用状态 / 接入节点 -->
      <el-divider content-position="left">基础信息</el-divider>
      <el-row :gutter="16">
        <el-col :span="16">
          <el-form-item label="入口名称">
            <el-input v-model="entryForm.name" placeholder="例如：Cloudflare WS 入口" />
          </el-form-item>
        </el-col>
        <el-col :span="8">
          <el-form-item label="启用状态">
            <el-switch v-model="entryForm.enabled" active-text="启用" inactive-text="停用" />
          </el-form-item>
        </el-col>
        <el-col :span="24">
          <el-form-item label="接入节点">
            <el-select
              v-model="entryForm.accessNodeId"
              filterable
              class="entry-field"
              @change="emit('access-node-change', $event)"
            >
              <el-option
                v-for="node in accessNodes"
                :key="node.id"
                :label="nodeOptionLabel(node)"
                :value="node.id"
              >
                <span>{{ node.name || node.publicHost || node.id }}</span>
                <el-tag v-if="node.cfEnabled" size="small" class="node-cf-tag">CF</el-tag>
              </el-option>
            </el-select>
          </el-form-item>
        </el-col>
      </el-row>

      <!-- ② 连接方式：分段控件（IP/域名/CF）+ 提示 +（域名/CF 时）选用域名 -->
      <el-divider content-position="left">连接方式</el-divider>
      <el-row :gutter="16">
        <el-col :span="24">
          <el-form-item label="连接方式">
            <el-radio-group
              v-model="entryForm.connectionMode"
              class="connection-mode-group"
              @change="handleConnectionModeChange"
            >
              <el-radio-button
                v-for="option in connectionModeOptions"
                :key="option.value"
                :value="option.value"
              >
                {{ option.label }}
              </el-radio-button>
            </el-radio-group>
            <small class="field-hint">{{ addressTypeHint }}</small>
          </el-form-item>
        </el-col>
        <el-col v-if="entryForm.connectionMode !== 'ip'" :span="12">
          <el-form-item label="选用域名">
            <el-select
              v-model="entryForm.nodeDomainId"
              class="entry-field"
              placeholder="按连接方式选用对应类型的节点域名"
              @change="handleNodeDomainChange"
            >
              <el-option
                v-for="domain in connectionModeDomainOptions"
                :key="domain.id"
                :label="nodeDomainOptionLabel(domain)"
                :value="domain.id"
              />
            </el-select>
            <small class="field-hint">监听地址默认带出所选域名，可改。</small>
          </el-form-item>
        </el-col>
      </el-row>

      <!-- ③ 监听：地址 / 端口（端口默认智能填未占用值，CF 限 CF 端口；见 EntryListenFields）。 -->
      <el-divider content-position="left">监听</el-divider>
      <EntryListenFields
        :form="entryForm"
        :is-cf="isCfAddress"
        :port-error="portError"
        @listen-host-input="emit('listen-host-input')"
      />

      <!-- ④ 协议与传输：协议 / 网络模式 / 安全 / 路径 / Host / Server Name -->
      <el-divider content-position="left">协议与传输</el-divider>
      <small class="field-hint section-hint">{{ addressTypeHint }}</small>
      <EntryProtocolFields
        :form="entryForm"
        :protocol-options="availableProtocolOptions"
        :transport-options="transportOptions"
        :carriage-options="carriageOptions"
        :needs-carriage="needsCarriage"
        :security-options="securityOptions"
        :needs-path="needsTransportPath"
        :needs-host="needsTransportHost"
        :path-label="transportPathLabel"
        :path-placeholder="transportPathPlaceholder"
        @protocol-change="handleProtocolChange"
        @security-change="handleSecurityChange"
        @network-change="normalizeEntryProtocolFields"
        @server-name-input="emit('server-name-input')"
        @transport-path-input="emit('transport-path-input')"
        @transport-host-input="emit('transport-host-input')"
      />

      <!-- ⑤ CDN：仅「CF 直连」连接方式显示。选 CF 直连即自动开启 CDN，这里只填 Cloudflare 字段；IP/域名直连不显示。 -->
      <template v-if="entryForm.connectionMode === 'cf'">
        <el-divider content-position="left">CDN（Cloudflare）</el-divider>
        <el-alert
          class="node-cf-hint"
          type="info"
          show-icon
          :closable="false"
          title="CF 直连已自动开启 CDN（橙云代理藏中转 IP），放开到 VLESS / Trojan 的 WS/gRPC/XHTTP+TLS；优先 DNS-01 给 CF 域名签自己的证书，无 token 时兜底复用直连证书（需 CF SSL=Full）。"
        />
        <el-row :gutter="16">
          <EntryCdnFields
            :form="entryForm"
            @cdn-hostname-input="emit('cdn-hostname-input')"
          />
        </el-row>
      </template>

      <!-- ⑥ 排序 -->
      <el-divider content-position="left">排序</el-divider>
      <el-row :gutter="16">
        <el-col :span="12">
          <el-form-item label="排序权重">
            <el-input-number
              v-model="entryForm.sortWeight"
              :min="0"
              :max="100000"
              class="entry-field"
            />
          </el-form-item>
        </el-col>
      </el-row>
    </el-form>
    <template #footer>
      <el-button @click="emit('update:modelValue', false)">取消</el-button>
      <el-button type="primary" :loading="saving" @click="emit('save')">保存</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.entry-field {
  width: 100%;
}

/* 连接方式分段控件：占满整行、按钮均分，让三档（IP/域名/CF）更醒目。 */
.connection-mode-group {
  display: flex;
  width: 100%;
}

.connection-mode-group :deep(.el-radio-button) {
  flex: 1;
}

.connection-mode-group :deep(.el-radio-button__inner) {
  width: 100%;
}

.node-cf-hint {
  margin-bottom: 12px;
}

.field-hint {
  color: var(--el-text-color-secondary);
  display: block;
  line-height: 1.4;
  margin-top: 4px;
}

/* 区块级提示（如「协议与传输」下方的地址类型说明）：与分隔线拉开间距。 */
.section-hint {
  margin-bottom: 12px;
  margin-top: -4px;
}

.node-cf-tag {
  margin-left: 6px;
}
</style>
