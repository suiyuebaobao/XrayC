<!-- 正式版本、后台任务与节点回报分开展示，未知版本不会显示为已核对。 -->
<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue';
import { computed, onMounted, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient } from '@/services/api';
import type { SystemInfo } from '@/services/api/systemInfo';
const info = ref<SystemInfo>();
const loading = ref(false);
const error = ref('');
const frontendRelease = import.meta.env.VITE_BUILD_ID || 'development';
const activeWorkers = computed(() => info.value?.workers.filter((worker) => worker.fresh) ?? []);
const matched = computed(() => Boolean(info.value && info.value.api.releaseId !== 'development'
  && info.value.api.releaseId === frontendRelease && activeWorkers.value.length
  && activeWorkers.value.every((worker) => worker.releaseId === info.value?.api.releaseId)));
onMounted(load);
async function load() {
  loading.value = true; error.value = '';
  try { info.value = await apiClient.getSystemInfo(); }
  catch (reason) { info.value = undefined; error.value = reason instanceof Error ? reason.message : '版本信息读取失败'; }
  finally { loading.value = false; }
}
</script>
<template>
  <PageHeader title="版本与运行" description="核对控制面版本、后台任务实例及节点安装回报。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
  </PageHeader>
  <el-alert v-if="error" :title="error" type="error" :closable="false" />
  <el-skeleton v-if="loading && !info" :rows="7" animated />
  <template v-else-if="info">
    <el-alert :type="matched ? 'success' : 'warning'" :closable="false" :title="matched ? '前端、API 与在线 Worker 的发布版本一致' : '版本尚未全部核对一致，请检查下方运行信息'" />
    <div class="version-grid">
      <el-card shadow="never"><span>前端</span><strong>{{ frontendRelease }}</strong></el-card>
      <el-card shadow="never"><span>API</span><strong>{{ info.api.releaseId }}</strong><small>{{ info.api.packageVersion }} · {{ info.api.platform }}</small><small>运行模式：{{ info.api.environment }}</small></el-card>
      <el-card shadow="never"><span>在线 Worker</span><strong>{{ activeWorkers.length }}</strong><small>依据实例心跳判断</small></el-card>
    </div>
    <el-card shadow="never" class="section-row">
      <template #header>Worker 实例</template>
      <el-table :data="info.workers" empty-text="尚未收到 Worker 回报">
        <el-table-column label="实例" min-width="160"><template #default="{ row }">{{ row.instanceId.slice(0, 8) }}</template></el-table-column>
        <el-table-column prop="releaseId" label="发布版本" min-width="220" />
        <el-table-column prop="heartbeatAt" label="最近回报" min-width="220" />
        <el-table-column label="状态" width="120"><template #default="{ row }"><el-tag :type="row.fresh ? 'success' : 'info'">{{ row.fresh ? '在线' : '停止回报' }}</el-tag></template></el-table-column>
      </el-table>
    </el-card>
    <el-card shadow="never" class="section-row">
      <template #header>节点版本</template>
      <p class="muted">Agent 版本来自心跳；Xray 版本由安装时识别并回报。未提供版本的旧节点明确显示“未上报”。</p>
      <el-table :data="info.nodes" empty-text="尚未接入节点">
        <el-table-column prop="name" label="节点" min-width="180" />
        <el-table-column label="Agent" min-width="210"><template #default="{ row }">{{ row.agentVersion || '未上报' }}</template></el-table-column>
        <el-table-column label="Xray 安装回报" min-width="260"><template #default="{ row }">{{ row.xrayVersion || '未上报' }}</template></el-table-column>
        <el-table-column prop="lastHeartbeatAt" label="最近心跳" min-width="220" />
      </el-table>
    </el-card>
  </template>
</template>
<style scoped>
.version-grid { display: grid; grid-template-columns: repeat(3,minmax(0,1fr)); gap: 16px; margin-top: 18px; }
.version-grid span, .version-grid small { display: block; color: var(--ink-soft); }
.version-grid strong { display: block; font-size: 20px; margin: 14px 0 8px; overflow-wrap: anywhere; }
@media(max-width:760px) { .version-grid { grid-template-columns: 1fr; } }
</style>
