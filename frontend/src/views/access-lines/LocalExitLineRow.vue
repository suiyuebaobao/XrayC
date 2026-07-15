<!--
  本子组件渲染「本机出口服务」批量创建里的单条线路表单（从 LocalExitLinesDialog 拆出以守 550 行）。
  连接方式用分段按钮（el-radio-button）置顶醒目，由它驱动域名/协议过滤；字段按视觉分区分组排版。
  纯展示与排版：所有连接方式驱动的响应式逻辑仍在父组件，子组件只透传 v-model 与 @change 事件，数据流不变。
  line 是父级传入的同一个响应式对象，直接改其字段即可双向同步；各 @change 由父用现有处理函数执行。
-->
<script setup lang="ts">
import { decoyDomainGroups } from '@/views/access-lines/decoyDomains';
import {
  localExitConnectionModeLabels,
  localExitShadowsocksMethodOptions,
  type LocalExitConnectionMode,
  type LocalExitLineFormWithMode,
} from '@/views/access-lines/localExitLines';

type SelectOption = { label: string; value: string; disabled?: boolean };
type DomainOption = { id: string; domain: string; isPrimary: boolean };

defineProps<{
  line: LocalExitLineFormWithMode;
  index: number;
  // 所属节点的公网出口 IP（优先 ip_direct_address，无则 public_host）：本机出口实际从这里出网，
  // 仅作参考展示。表单里的「地址」字段是本机出口服务的连接地址（多为 127.0.0.1 回环），与之不同。
  nodeExitIp: string;
  // 由父级按节点可用连接方式算出（IP 直连 / 域名直连）；本机出口无 CF 档。
  connectionModeOptions: Array<{ label: string; value: LocalExitConnectionMode }>;
  // 以下选项均由父级按当前行连接方式/协议/安全模式过滤后传入，保持与后端护栏同口径。
  domainOptions: DomainOption[];
  protocolOptions: SelectOption[];
  vlessSecurityOptions: SelectOption[];
  // 传输（可多选，每个传输 = 一条线路）；承载（RAW 传输的 L4 单选，SS/SOCKS 可合并 tcp,udp）。
  transportOptions: SelectOption[];
  carriageOptions: SelectOption[];
  // 仅当选中 RAW(tcp) 传输时才展示承载下拉（HY2 固定 udp 无 RAW 承载）。
  needsCarriage: boolean;
  // 端口即时校验文案：撞上节点已用端口（入口 / 已建本机出口）时红字提示，空串=无提示。
  portError: string;
}>();

const emit = defineEmits<{
  (event: 'remove'): void;
  // 各 @change/@blur 透传回父组件，由父调用现有处理函数（数据流不变）。
  (event: 'connection-mode-change'): void;
  (event: 'node-domain-change'): void;
  (event: 'protocol-change'): void;
  (event: 'network-change'): void;
  (event: 'vless-security-change'): void;
  (event: 'host-change'): void;
  (event: 'server-name-change'): void;
}>();

// 连接方式一行小灰字说明：让用户一眼知道每档放哪些协议、选了域名直连才出现「选用域名」。
const connectionModeHint =
  'IP 直连：免证书，可选 Reality / Shadowsocks / SOCKS5 / HTTP；域名直连：可选 Trojan / HY2 / VLESS-TLS，选中后请在右侧挑一个直连域名。';

function nodeDomainOptionLabel(domain: DomainOption) {
  return `${domain.domain}${domain.isPrimary ? '（主）' : ''}`;
}

function connectionModeLabel(mode: LocalExitConnectionMode) {
  return localExitConnectionModeLabels[mode];
}
</script>

<template>
  <section class="line-row">
    <header class="line-row__header">
      <strong class="line-row__title">线路 {{ index + 1 }}</strong>
      <el-button text type="danger" @click="emit('remove')">删除</el-button>
    </header>

    <!-- ① 连接方式：靠顶部、醒目的分段控件，选中后才显示「选用域名」。 -->
    <div class="line-section line-section--mode">
      <div class="line-section__head">
        <span class="line-section__title">连接方式</span>
        <span class="line-section__hint">{{ connectionModeHint }}</span>
      </div>
      <div class="mode-line">
        <el-radio-group
          v-model="line.connectionMode"
          class="mode-group"
          @change="emit('connection-mode-change')"
        >
          <el-radio-button
            v-for="option in connectionModeOptions"
            :key="option.value"
            :value="option.value"
            class="mode-button"
          >
            {{ connectionModeLabel(option.value) }}
          </el-radio-button>
        </el-radio-group>
        <div v-if="line.connectionMode !== 'ip'" class="mode-domain">
          <span class="mode-domain__label">选用域名</span>
          <el-select
            v-model="line.nodeDomainId"
            class="mode-domain__select"
            placeholder="选择一个直连域名"
            @change="emit('node-domain-change')"
          >
            <el-option
              v-for="domain in domainOptions"
              :key="domain.id"
              :label="nodeDomainOptionLabel(domain)"
              :value="domain.id"
            />
          </el-select>
        </div>
      </div>
    </div>

    <!-- ② 基础：线路名称 / 协议档名称。 -->
    <div class="line-section">
      <span class="line-section__title">基础</span>
      <el-row :gutter="16">
        <el-col :xs="24" :sm="12">
          <el-form-item label="线路名称">
            <el-input v-model="line.resourceName" placeholder="例如：香港自建 VLESS" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :sm="12">
          <el-form-item label="协议档名称">
            <el-input v-model="line.endpointName" placeholder="为空时使用线路名称" />
          </el-form-item>
        </el-col>
      </el-row>
    </div>

    <!-- ③ 协议与传输：线路协议 / 网络模式 / VLESS 安全 / 地址 / 端口。 -->
    <div class="line-section">
      <span class="line-section__title">协议与传输</span>
      <!-- 协议/传输/承载/安全：统一为一排等宽下拉（6 宽 4 列），条件项左对齐、不撑高邻格。 -->
      <el-row :gutter="16">
        <el-col :xs="24" :sm="6">
          <el-form-item label="线路协议">
            <el-select v-model="line.outboundType" class="line-field" @change="emit('protocol-change')">
              <el-option
                v-for="option in protocolOptions"
                :key="option.value"
                :label="option.label"
                :value="option.value"
              />
            </el-select>
          </el-form-item>
        </el-col>
        <el-col :xs="24" :sm="6">
          <el-form-item label="传输（可多选）">
            <el-select
              v-model="line.transports"
              class="line-field"
              multiple
              collapse-tags
              collapse-tags-tooltip
              @change="emit('network-change')"
            >
              <el-option
                v-for="option in transportOptions"
                :key="option.value"
                :label="option.label"
                :value="option.value"
                :disabled="option.disabled"
              />
            </el-select>
          </el-form-item>
        </el-col>
        <el-col v-if="needsCarriage" :xs="24" :sm="6">
          <el-form-item label="承载（TCP/UDP）">
            <el-select v-model="line.carriage" class="line-field" @change="emit('network-change')">
              <el-option
                v-for="option in carriageOptions"
                :key="option.value"
                :label="option.label"
                :value="option.value"
                :disabled="option.disabled"
              />
            </el-select>
          </el-form-item>
        </el-col>
        <el-col v-if="line.outboundType === 'vless'" :xs="24" :sm="6">
          <el-form-item label="VLESS 安全模式">
            <el-select v-model="line.vlessSecurity" class="line-field" @change="emit('vless-security-change')">
              <el-option
                v-for="option in vlessSecurityOptions"
                :key="option.value"
                :label="option.label"
                :value="option.value"
              />
            </el-select>
          </el-form-item>
        </el-col>
      </el-row>
      <!-- 地址/端口：独立一排；长说明移到整行下方，避免把地址格撑高造成上下参差。 -->
      <el-row :gutter="16">
        <el-col :xs="24" :sm="18">
          <el-form-item label="本机出口服务地址">
            <el-input
              v-model="line.host"
              placeholder="按连接方式自动带出（IP 直连=节点 IP，域名直连=所选域名），可改"
              @blur="emit('host-change')"
            />
          </el-form-item>
        </el-col>
        <el-col :xs="12" :sm="6">
          <el-form-item label="端口">
            <el-input-number v-model="line.port" :min="1" :max="65535" class="line-field" controls-position="right" />
            <small v-if="portError" class="line-field__error">{{ portError }}</small>
          </el-form-item>
        </el-col>
        <el-col :span="24">
          <div class="line-field__hint">
按连接方式带出地址：IP 直连填节点 IP、域名直连填所选域名（可改）<template v-if="nodeExitIp">；实际出网 IP <span class="line-field__exit-ip">{{ nodeExitIp }}</span></template><template v-else>；所属节点尚未配公网 IP，请先在节点补全</template>
          </div>
        </el-col>
      </el-row>
    </div>

    <!-- ④ 协议专属：UUID / SNI / 证书 / 密码 / 加密 / 凭据，随协议出现。 -->
    <div
      v-if="
        line.outboundType === 'vless' ||
        line.outboundType === 'hysteria' ||
        line.outboundType === 'trojan' ||
        line.outboundType === 'shadowsocks' ||
        line.outboundType === 'socks' ||
        line.outboundType === 'http'
      "
      class="line-section"
    >
      <span class="line-section__title">协议专属</span>
      <el-row :gutter="16">
        <template v-if="line.outboundType === 'vless'">
          <el-col :xs="24" :sm="12">
            <el-form-item label="UUID">
              <el-input v-model="line.uuid" placeholder="自动生成，可按需修改" />
            </el-form-item>
          </el-col>
          <!-- 伪装域名（仅 VLESS+Reality）：分类下拉 + 可手输。TLS（域名直连）SNI 自动=选用域名、none 无 SNI，均不单独显示。 -->
          <el-col v-if="line.vlessSecurity === 'reality'" :xs="24" :sm="12">
            <el-form-item label="伪装域名">
              <el-select
                v-model="line.serverName"
                filterable
                allow-create
                default-first-option
                class="line-field"
                placeholder="选择或输入伪装域名（Reality dest）"
                @change="emit('server-name-change')"
              >
                <el-option-group
                  v-for="group in decoyDomainGroups"
                  :key="group.label"
                  :label="group.label"
                >
                  <el-option
                    v-for="option in group.options"
                    :key="option.value"
                    :label="option.label"
                    :value="option.value"
                  />
                </el-option-group>
              </el-select>
            </el-form-item>
          </el-col>
        </template>
        <template v-if="line.outboundType === 'hysteria' || line.outboundType === 'trojan'">
          <!-- 域名直连 Trojan/HY2：SNI=证书域名（选用域名），自动随地址带出，不再单独显示输入框（功能二）。 -->
          <el-col :xs="24" :sm="8">
            <el-form-item label="密码">
              <el-input v-model="line.password" placeholder="自动生成，可按需修改" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="8">
            <el-form-item label="证书文件">
              <el-input v-model="line.tlsCertificateFile" placeholder="fullchain.pem 路径" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="8">
            <el-form-item label="私钥文件">
              <el-input v-model="line.tlsKeyFile" placeholder="privkey.pem 路径" />
            </el-form-item>
          </el-col>
        </template>
        <template v-if="line.outboundType === 'shadowsocks'">
          <el-col :xs="24" :sm="12">
            <el-form-item label="加密方法">
              <el-select v-model="line.method" class="line-field">
                <el-option
                  v-for="option in localExitShadowsocksMethodOptions"
                  :key="option.value"
                  :label="option.label"
                  :value="option.value"
                />
              </el-select>
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="12">
            <el-form-item label="密码">
              <el-input v-model="line.password" placeholder="自动生成，可按需修改" />
            </el-form-item>
          </el-col>
        </template>
        <template v-if="line.outboundType === 'socks' || line.outboundType === 'http'">
          <el-col :xs="24" :sm="12">
            <el-form-item label="用户名">
              <el-input v-model="line.username" placeholder="可留空" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="12">
            <el-form-item label="密码">
              <el-input v-model="line.password" placeholder="可留空" />
            </el-form-item>
          </el-col>
        </template>
      </el-row>
    </div>

    <!-- 状态：单列开关，与上方分区留出间距。 -->
    <div class="line-section line-section--state">
      <el-form-item label="状态">
        <el-switch v-model="line.enabled" active-text="启用" inactive-text="停用" />
      </el-form-item>
    </div>
  </section>
</template>

<style scoped>
.line-row {
  border: 1px solid var(--el-border-color);
  border-radius: 10px;
  padding: 12px 14px 0;
  background: var(--el-fill-color-blank);
}

/* 紧凑化：收紧表单行距与顶部标签间距，让每条线路更密。 */
.line-row :deep(.el-form-item) {
  margin-bottom: 8px;
}

.line-row :deep(.el-form-item__label) {
  padding-bottom: 2px;
  line-height: 1.3;
}

.line-row__header {
  align-items: center;
  display: flex;
  justify-content: space-between;
  margin-bottom: 8px;
}

.line-row__title {
  font-size: 15px;
  color: var(--el-text-color-primary);
}

.line-section {
  margin-bottom: 8px;
}

.line-section__head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px;
  margin-bottom: 6px;
}

.line-section__title {
  display: block;
  font-size: 12px;
  font-weight: 600;
  color: var(--el-text-color-secondary);
  margin-bottom: 4px;
}

.line-section__head .line-section__title {
  margin-bottom: 0;
}

.line-section__hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  line-height: 1.5;
}

/* 连接方式分区：轻底色作锚点，瘦身去重边框、收紧内边距。 */
.line-section--mode {
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--el-fill-color-light);
}

.mode-line {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
}

.mode-group {
  flex-wrap: wrap;
}

.mode-button :deep(.el-radio-button__inner) {
  min-width: 96px;
  font-weight: 500;
}

.mode-domain {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1 1 260px;
  min-width: 220px;
}

.mode-domain__label {
  font-size: 13px;
  color: var(--el-text-color-regular);
  white-space: nowrap;
}

.mode-domain__select {
  flex: 1 1 auto;
  min-width: 160px;
}

.line-section--state {
  margin-bottom: 0;
}

.line-field {
  width: 100%;
}

.line-field__hint {
  margin-top: 4px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--el-text-color-secondary);
}

.line-field__exit-ip {
  color: var(--el-text-color-primary);
  font-weight: 600;
}

/* 端口即时校验红字：撞上节点已用端口时提示，提交前就拦住。 */
.line-field__error {
  margin-top: 4px;
  display: block;
  font-size: 12px;
  line-height: 1.4;
  color: var(--el-color-danger);
}
</style>
