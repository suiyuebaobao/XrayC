<!-- 两个出口入口共用编辑与保存逻辑。本机出口保留证书/节点归属校验，不转走第三方出口接口。 -->
<script setup lang="ts">
import { ElMessage } from 'element-plus';
import { computed, reactive, ref, watch } from 'vue';
import { apiClient, type ExitEndpointSummary, type ExitResourceSummary, type NodeDomain, type ThirdPartyExitEndpointOutboundType } from '@/services/api';
import { defaultOutboundConfig, ensureNoHongKongHysteria, outboundOptionsForRegion, parseJsonObject, protocolConfigHint, validateProtocolConfig, vlessSecurityFromConfigText, setVlessOutboundSecurity, vlessSecurityOptions } from '@/views/exit-pools/exitPoolsPageHelpers';
import { localExitVlessSecurityOptionsForDomainKind } from '@/views/access-lines/localExitLines';
import { formatJson } from '@/views/line-pool/format';

const visible = defineModel<boolean>({ required: true });
const props = defineProps<{
  endpointId: string;
  initialEndpoint?: ExitEndpointSummary;
  initialResource?: ExitResourceSummary;
  node?: { id: string; domains: NodeDomain[] } | null;
}>();
const emit = defineEmits<{ changed: [] }>();
const loading = ref(false);
const saving = ref(false);
const loadError = ref('');
const endpoint = ref<ExitEndpointSummary>();
const resource = ref<ExitResourceSummary>();
const domains = ref<NodeDomain[]>([]);
const isLocal = computed(() => resource.value?.ownership === 'self_hosted');
const title = computed(() => isLocal.value ? '编辑本机出口线路' : '查看/编辑线路');
const form = reactive({ name: '', host: '', port: 443, enabled: true, region: '', provider: '',
  outboundType: 'socks' as ThirdPartyExitEndpointOutboundType, nodeDomainId: '',
  outboundConfig: '{}', streamConfig: '{}', probeConfig: '{}' });
const vlessSecurity = computed({
  get: () => vlessSecurityFromConfigText(form.outboundConfig),
  set: (value: 'none' | 'tls' | 'reality') => { form.outboundConfig = setVlessOutboundSecurity(form.outboundConfig, value); },
});
const securityOptions = computed(() => isLocal.value
  ? localExitVlessSecurityOptionsForDomainKind(form.nodeDomainId ? 'direct' : '') : vlessSecurityOptions);
let baseline = { ...form };
let loadGeneration = 0;

watch(() => [visible.value, props.endpointId], async () => {
  const generation = ++loadGeneration;
  if (!visible.value || !props.endpointId) return;
  loading.value = true; loadError.value = ''; endpoint.value = undefined; resource.value = undefined;
  try {
    const [endpoints, resources] = await Promise.all([
      props.initialEndpoint ? Promise.resolve([props.initialEndpoint]) : apiClient.getExitEndpoints(),
      props.initialResource ? Promise.resolve([props.initialResource]) : apiClient.getExitResources(),
    ]);
    const selected = endpoints.find((item) => item.id === props.endpointId);
    const owner = resources.find((item) => item.id === selected?.exitResourceId);
    if (!selected || !owner) throw new Error('出口已不存在，请刷新列表。');
    if (selected.outboundType === 'direct') throw new Error('历史 direct 线路只读，请新建普通线路替换。');
    let nodeDomainId = '';
    let nodeDomains: NodeDomain[] = [];
    if (owner.ownership === 'self_hosted') {
      if (!owner.accessNodeId) throw new Error('本机出口缺少所属节点。');
      const [localLines, nodes] = await Promise.all([
        apiClient.listLocalExitLines(owner.accessNodeId),
        props.node?.id === owner.accessNodeId ? Promise.resolve([props.node])
          : apiClient.getControlPlane().then((data) => data.accessNodes),
      ]);
      const local = localLines.find((item) => item.exitEndpointId === selected.id);
      if (!local) throw new Error('本机出口归属已改变，请刷新列表。');
      nodeDomainId = local.nodeDomainId;
      nodeDomains = nodes.find((item) => item.id === owner.accessNodeId)?.domains ?? [];
    }
    if (generation !== loadGeneration) return;
    endpoint.value = selected; resource.value = owner; domains.value = nodeDomains;
    Object.assign(form, { name: selected.name || owner.name, host: selected.host, port: selected.port,
      enabled: selected.enabled && owner.enabled, region: owner.region, provider: owner.providerName,
      outboundType: selected.outboundType, nodeDomainId,
      outboundConfig: formatJson(selected.outboundConfig), streamConfig: formatJson(selected.streamConfig),
      probeConfig: formatJson(selected.probeConfig) });
    baseline = { ...form };
  } catch (error) {
    if (generation === loadGeneration) loadError.value = error instanceof Error ? error.message : '加载出口失败';
  } finally { if (generation === loadGeneration) loading.value = false; }
}, { immediate: true });

function changeProtocol(value: ThirdPartyExitEndpointOutboundType) {
  form.outboundConfig = defaultOutboundConfig(value); form.streamConfig = '{}';
}

async function save() {
  if (!endpoint.value || !resource.value) return;
  saving.value = true;
  let endpointSaved = false;
  try {
    if (!form.name.trim() || !form.host.trim()) throw new Error('线路名称和地址不能为空。');
    if (!Number.isInteger(form.port) || form.port < 1 || form.port > 65535) throw new Error('端口必须在 1-65535 之间');
    ensureNoHongKongHysteria(form.outboundType, form.region);
    const outbound = form.outboundConfig !== baseline.outboundConfig ? parseJsonObject(form.outboundConfig) : undefined;
    if (outbound) validateProtocolConfig(form.outboundType, outbound);
    const connection = {
      host: form.host !== baseline.host ? form.host.trim() : undefined,
      port: form.port !== baseline.port ? form.port : undefined,
      enabled: form.enabled !== baseline.enabled ? form.enabled : undefined,
      outbound_config: outbound,
      stream_config: form.streamConfig !== baseline.streamConfig ? parseJsonObject(form.streamConfig) : undefined,
    };
    const probeConfig = form.probeConfig !== baseline.probeConfig ? parseJsonObject(form.probeConfig) : undefined;
    const name = form.name !== baseline.name ? form.name.trim() : undefined;
    const resourcePatch = {
      name, region_code: form.region !== baseline.region ? form.region.trim() : undefined,
      provider_name: form.provider !== baseline.provider ? form.provider.trim() : undefined,
      enabled: connection.enabled,
    };
    if (isLocal.value) {
      const localPatch = { ...connection, endpoint_name: name,
        node_domain_id: form.nodeDomainId !== baseline.nodeDomainId ? form.nodeDomainId || null : undefined };
      if (Object.values(localPatch).some((value) => value !== undefined)) {
        await apiClient.updateLocalExitLine(resource.value.accessNodeId, endpoint.value.id, localPatch);
        endpointSaved = true;
      }
    } else {
      const patch = { ...connection, name,
        outbound_type: form.outboundType !== baseline.outboundType ? form.outboundType : undefined,
        probe_config: probeConfig };
      if (Object.values(patch).some((value) => value !== undefined)) {
        await apiClient.updateExitEndpoint(endpoint.value.id, patch);
        endpointSaved = true;
      }
    }
    if (isLocal.value && probeConfig) {
      await apiClient.updateExitEndpoint(endpoint.value.id, { probe_config: probeConfig });
      endpointSaved = true;
    }
    if (Object.values(resourcePatch).some((value) => value !== undefined)) {
      await apiClient.updateExitResource(resource.value.id, resourcePatch);
    }
    visible.value = false; emit('changed');
    ElMessage.success('出口设置已保存，关联节点将同步更新');
  } catch (error) {
    const message = error instanceof Error ? error.message : '保存出口失败';
    if (endpointSaved) emit('changed');
    ElMessage.error(endpointSaved ? `部分设置已保存，后续保存失败：${message}` : message);
  } finally { saving.value = false; }
}
</script>

<template>
  <el-dialog v-model="visible" :title="title" width="760px" append-to-body :close-on-click-modal="!saving" destroy-on-close>
    <el-skeleton v-if="loading" :rows="7" animated />
    <el-alert v-else-if="loadError" :title="loadError" type="error" :closable="false" />
    <el-form v-else label-position="top">
      <el-row :gutter="16">
        <el-col :xs="24" :sm="12"><el-form-item :label="isLocal ? '协议档名称' : '线路名称'"><el-input v-model="form.name" /></el-form-item></el-col>
        <el-col :xs="24" :sm="12"><el-form-item label="协议">
          <el-input v-if="isLocal" :model-value="form.outboundType" disabled />
          <el-select v-else v-model="form.outboundType" @change="changeProtocol">
            <el-option v-for="option in outboundOptionsForRegion(form.region)" :key="option.value" :label="option.label" :value="option.value" :disabled="option.disabled" />
          </el-select>
        </el-form-item></el-col>
        <el-col v-if="form.outboundType === 'vless'" :xs="24" :sm="12"><el-form-item label="VLESS 安全模式"><el-select v-model="vlessSecurity">
          <el-option v-for="option in securityOptions" :key="option.value" :label="option.label" :value="option.value" />
        </el-select></el-form-item></el-col>
        <el-col :xs="24" :sm="16"><el-form-item label="地址"><el-input v-model="form.host" /></el-form-item></el-col>
        <el-col :xs="24" :sm="8"><el-form-item label="端口"><el-input-number v-model="form.port" :min="1" :max="65535" /></el-form-item></el-col>
        <el-col v-if="isLocal" :xs="24" :sm="16"><el-form-item label="选用域名"><el-select v-model="form.nodeDomainId" clearable placeholder="留空=免证书">
          <el-option v-for="domain in domains.filter((item) => item.kind === 'direct')" :key="domain.id" :value="domain.id" :label="domain.domain" />
        </el-select></el-form-item></el-col>
        <el-col :xs="24" :sm="8"><el-form-item label="状态"><el-switch v-model="form.enabled" active-text="启用" inactive-text="停用" /></el-form-item></el-col>
      </el-row>
      <el-collapse>
        <el-collapse-item name="advanced" title="高级连接设置">
          <el-row :gutter="16">
            <el-col :xs="24" :sm="12"><el-form-item label="地区代码"><el-input v-model="form.region" /></el-form-item></el-col>
            <el-col :xs="24" :sm="12"><el-form-item label="供应商"><el-input v-model="form.provider" /></el-form-item></el-col>
          </el-row>
          <el-form-item label="连接配置 JSON"><el-input v-model="form.outboundConfig" type="textarea" :rows="6" :placeholder="protocolConfigHint(form.outboundType)" /></el-form-item>
          <el-form-item label="Stream 配置 JSON"><el-input v-model="form.streamConfig" type="textarea" :rows="4" /></el-form-item>
          <el-form-item label="探测配置 JSON"><el-input v-model="form.probeConfig" type="textarea" :rows="3" /></el-form-item>
        </el-collapse-item>
      </el-collapse>
    </el-form>
    <template #footer>
      <el-button :disabled="saving" @click="visible = false">取消</el-button>
      <el-button type="primary" :disabled="loading || Boolean(loadError) || !endpoint" :loading="saving" @click="save">保存</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
:deep(.el-select), :deep(.el-input-number) { width: 100%; }
</style>
