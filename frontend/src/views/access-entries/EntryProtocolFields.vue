<!--
  本组件承载入口管理「协议与传输」区块字段，从 AccessEntryDialog 拆出以守 550 行上限。
  负责协议 / 网络模式 / 安全模式 / 路径 / Host / Server Name 的展示与双向绑定。
  所有可用选项、标签、占位符、是否展示路径/Host 等护栏派生仍在父级计算后以 prop 传入，
  组件只做展示与就地改写父级 reactive 表单字段，不持有页面状态、不请求接口、不改护栏逻辑。
-->
<script setup lang="ts">
import { computed, watch } from 'vue';
import { decoyDomainGroups } from '@/views/access-lines/decoyDomains';

interface EntryProtocolFormState {
  protocol: string;
  transports: string[];
  carriage: string;
  security: string;
  serverName: string;
  // VLESS 量子加密（后量子）开关；仅非 Reality VLESS 显示，切到 Reality/非 VLESS 自动置 false。
  vlessQuantumEncryption: boolean;
  wsPath: string;
  wsHost: string;
}

interface SelectOption {
  label: string;
  value: string;
}

// form 是父级 reactive 入口表单对象，直接就地改写其协议相关字段（与 EntryCdnFields 同口径）。
const props = defineProps<{
  form: EntryProtocolFormState;
  protocolOptions: SelectOption[];
  // 传输选项（streamSettings.network，可多选，每个传输 = 一条入口）。
  transportOptions: SelectOption[];
  // 承载选项（L4 单选合并，仅 RAW 传输生效）。
  carriageOptions: SelectOption[];
  // 是否展示承载下拉：仅当传输含 RAW(tcp) 时才有意义（HY2 / 纯 WS/gRPC/XHTTP 不需要）。
  needsCarriage: boolean;
  securityOptions: SelectOption[];
  needsPath: boolean;
  needsHost: boolean;
  pathLabel: string;
  pathPlaceholder: string;
}>();

const emit = defineEmits<{
  // 协议变化：父级据此重新归一安全/网络/端口护栏。
  (event: 'protocol-change'): void;
  // 安全模式变化：父级据此重新归一网络模式。
  (event: 'security-change'): void;
  // 传输多选 / 承载单选变化：父级据此归一传输集合与承载。
  (event: 'network-change'): void;
  // 管理员手改 Server Name / 路径 / Host，页面据此标脏不再自动覆盖默认值。
  (event: 'server-name-input'): void;
  (event: 'transport-path-input'): void;
  (event: 'transport-host-input'): void;
}>();

// 伪装域名字段仅 VLESS+Reality 显示（分类下拉，见功能三）：TLS 的 SNI 自动=选用域名、none 无 SNI，均不单独显示。
const showDecoyDomain = computed(
  () => props.form.protocol === 'vless' && props.form.security === 'reality',
);
// 量子加密开关仅非 Reality VLESS 显示（security=tls / 空 none，含 CF）；Reality 隐藏（靠选后量子 dest）。
const showQuantumSwitch = computed(
  () => props.form.protocol === 'vless' && props.form.security !== 'reality',
);

// 切到 Reality 或非 VLESS 协议时自动关掉量子开关，避免隐藏后仍带脏值提交（规则：自动置 false）。
watch(showQuantumSwitch, (visible) => {
  if (!visible && props.form.vlessQuantumEncryption) {
    props.form.vlessQuantumEncryption = false;
  }
});
</script>

<template>
  <el-row :gutter="16">
    <el-col :span="8">
      <el-form-item label="入口/伪装协议">
        <el-select v-model="form.protocol" class="entry-protocol-field" @change="emit('protocol-change')">
          <el-option
            v-for="option in protocolOptions"
            :key="option.value"
            :label="option.label"
            :value="option.value"
          />
        </el-select>
      </el-form-item>
    </el-col>
    <el-col :span="8">
      <el-form-item label="传输（可多选）">
        <el-select
          v-model="form.transports"
          multiple
          collapse-tags
          collapse-tags-tooltip
          class="entry-protocol-field"
          @change="emit('network-change')"
        >
          <el-option
            v-for="option in transportOptions"
            :key="option.value"
            :label="option.label"
            :value="option.value"
          />
        </el-select>
      </el-form-item>
    </el-col>
    <el-col v-if="needsCarriage" :span="8">
      <el-form-item label="承载（TCP/UDP）">
        <el-select v-model="form.carriage" class="entry-protocol-field" @change="emit('network-change')">
          <el-option
            v-for="option in carriageOptions"
            :key="option.value"
            :label="option.label"
            :value="option.value"
          />
        </el-select>
      </el-form-item>
    </el-col>
    <el-col :span="8">
      <el-form-item label="安全模式">
        <el-select v-model="form.security" class="entry-protocol-field" @change="emit('security-change')">
          <el-option
            v-for="option in securityOptions"
            :key="option.value"
            :label="option.label"
            :value="option.value"
          />
        </el-select>
      </el-form-item>
    </el-col>
    <el-col v-if="needsPath" :span="12">
      <el-form-item :label="pathLabel">
        <el-input v-model="form.wsPath" :placeholder="pathPlaceholder" @input="emit('transport-path-input')" />
      </el-form-item>
    </el-col>
    <el-col v-if="needsHost" :span="12">
      <el-form-item label="Host">
        <el-input
          v-model="form.wsHost"
          placeholder="可选，默认使用 SNI 或 CDN Hostname"
          @input="emit('transport-host-input')"
        />
      </el-form-item>
    </el-col>
    <!-- 伪装域名（仅 VLESS+Reality）：分类下拉 + 可手输（allow-create），默认 www.cloudflare.com。 -->
    <el-col v-if="showDecoyDomain" :span="12">
      <el-form-item label="伪装域名">
        <el-select
          v-model="form.serverName"
          filterable
          allow-create
          default-first-option
          class="entry-protocol-field"
          placeholder="选择或输入伪装域名（Reality dest）"
          @change="emit('server-name-input')"
        >
          <el-option-group v-for="group in decoyDomainGroups" :key="group.label" :label="group.label">
            <el-option
              v-for="option in group.options"
              :key="option.value"
              :label="option.label"
              :value="option.value"
            />
          </el-option-group>
        </el-select>
        <small class="entry-protocol-hint">Reality 伪装目标域名，默认 www.cloudflare.com；可手输列表外域名。</small>
      </el-form-item>
    </el-col>
    <!-- 量子加密（后量子）：仅非 Reality VLESS 显示；TLS 的 SNI 已并入选用域名、none 无 SNI，故此处不再有独立 Server Name 框。 -->
    <el-col v-if="showQuantumSwitch" :span="12">
      <el-form-item label="量子加密">
        <el-switch v-model="form.vlessQuantumEncryption" />
        <small class="entry-protocol-hint">开启后仅 Xray/mihomo 系客户端可用，sing-box 系连不上。</small>
      </el-form-item>
    </el-col>
  </el-row>
</template>

<style scoped>
.entry-protocol-field {
  width: 100%;
}

.entry-protocol-hint {
  color: var(--el-text-color-secondary);
  display: block;
  line-height: 1.4;
  margin-top: 4px;
}
</style>
