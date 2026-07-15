<!--
  本页面用于后台配置订阅 YAML 的基础字段。
  客户端分组不在这里维护，直接由套餐授权的分组生成。
  订阅输出不携带图标字段，节点和分组名称保持纯文本。
-->
<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type SubscriptionSettings } from '@/services/api';

const loading = ref(true);
const saving = ref(false);

const settings = reactive<SubscriptionSettings>({
  mixedPort: 7890,
  allowLan: false,
  mode: 'rule',
  logLevel: 'info',
  profileName: 'XrayC',
  updateIntervalHours: 24,
  defaultRules: [],
  autoTestEnabled: false,
  autoTestName: '自动选择',
  autoTestUrl: 'http://cp.cloudflare.com/generate_204',
  autoTestIntervalSeconds: 86400,
  blockUnhealthyLines: false,
});

onMounted(load);

async function load() {
  loading.value = true;
  try {
    applySettings(await apiClient.getSubscriptionSettings());
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载订阅设置失败');
  } finally {
    loading.value = false;
  }
}

async function save() {
  saving.value = true;
  try {
    applySettings(await apiClient.updateSubscriptionSettings({ ...settings }));
    ElMessage.success('订阅设置已保存');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存订阅设置失败');
  } finally {
    saving.value = false;
  }
}

function applySettings(next: SubscriptionSettings) {
  settings.mixedPort = next.mixedPort;
  settings.allowLan = next.allowLan;
  settings.mode = next.mode === 'global' ? 'global' : 'rule';
  settings.logLevel = next.logLevel || 'info';
  settings.profileName = next.profileName || 'XrayC';
  settings.updateIntervalHours = next.updateIntervalHours || 24;
  settings.defaultRules = [...next.defaultRules];
  settings.autoTestEnabled = next.autoTestEnabled;
  settings.autoTestName = next.autoTestName || '自动选择';
  settings.autoTestUrl = next.autoTestUrl || 'http://cp.cloudflare.com/generate_204';
  settings.autoTestIntervalSeconds = next.autoTestIntervalSeconds || 86400;
  settings.blockUnhealthyLines = next.blockUnhealthyLines;
}

</script>

<template>
  <PageHeader title="订阅设置" description="配置 Clash/mihomo 基础字段；客户端分组由套餐授权的分组直接生成。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" :loading="saving" @click="save">保存配置</el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="10" animated />
  <el-row v-else :gutter="18">
    <el-col :xs="24" :lg="12">
      <el-card shadow="never">
        <template #header>基础配置</template>
        <el-form label-position="top">
          <el-form-item label="订阅名称">
            <el-input v-model="settings.profileName" placeholder="例如：XrayC" />
          </el-form-item>
          <div class="basic-grid">
            <el-form-item label="mixed-port">
              <el-input-number v-model="settings.mixedPort" :min="0" :max="65535" controls-position="right" />
            </el-form-item>
            <el-form-item label="allow-lan">
              <el-switch v-model="settings.allowLan" active-text="允许" inactive-text="关闭" />
            </el-form-item>
            <el-form-item label="mode">
              <el-select v-model="settings.mode" class="form-field">
                <el-option label="rule" value="rule" />
                <el-option label="global" value="global" />
              </el-select>
            </el-form-item>
            <el-form-item label="log-level">
              <el-select v-model="settings.logLevel" class="form-field">
                <el-option label="info" value="info" />
                <el-option label="warning" value="warning" />
                <el-option label="error" value="error" />
                <el-option label="debug" value="debug" />
                <el-option label="silent" value="silent" />
              </el-select>
            </el-form-item>
            <el-form-item label="更新间隔">
              <el-input-number
                v-model="settings.updateIntervalHours"
                :min="1"
                :max="168"
                controls-position="right"
              />
              <span class="field-suffix">小时</span>
            </el-form-item>
          </div>
        </el-form>
      </el-card>
    </el-col>

    <el-col :xs="24" :lg="12">
      <el-card shadow="never">
        <template #header>线路过滤</template>
        <el-alert
          title="健康检查只负责标记线路状态；默认不会因为探测故障把线路从订阅里隐藏。打开下方开关后，探测离线的线路才会被屏蔽。"
          type="info"
          show-icon
          :closable="false"
          class="settings-alert"
        />
        <el-form label-position="top">
          <el-form-item label="屏蔽故障线路">
            <el-switch
              v-model="settings.blockUnhealthyLines"
              active-text="屏蔽"
              inactive-text="继续显示"
            />
          </el-form-item>
        </el-form>
      </el-card>

      <el-card shadow="never" class="settings-card">
        <template #header>自动测速</template>
        <el-alert
          title="普通选择分组会按套餐授权的分组自动生成；这里仅控制是否额外生成一个全节点 url-test 分组。"
          type="info"
          show-icon
          :closable="false"
          class="settings-alert"
        />
        <el-form label-position="top">
          <el-form-item label="启用自动测速分组">
            <el-switch v-model="settings.autoTestEnabled" active-text="启用" inactive-text="关闭" />
          </el-form-item>
          <el-form-item label="测速分组名称">
            <el-input v-model="settings.autoTestName" :disabled="!settings.autoTestEnabled" />
          </el-form-item>
          <el-form-item label="测速 URL">
            <el-input v-model="settings.autoTestUrl" :disabled="!settings.autoTestEnabled" />
          </el-form-item>
          <el-form-item label="测速间隔">
            <el-input-number
              v-model="settings.autoTestIntervalSeconds"
              :min="1"
              :max="86400"
              :disabled="!settings.autoTestEnabled"
              controls-position="right"
            />
            <span class="field-suffix">秒</span>
          </el-form-item>
        </el-form>
      </el-card>
    </el-col>
  </el-row>
</template>

<style scoped>
.basic-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0 14px;
}

.form-field {
  width: 100%;
}

.field-suffix {
  margin-left: 8px;
  color: var(--ink-soft);
}

.settings-alert {
  margin-bottom: 16px;
}

.settings-card {
  margin-top: 18px;
}

@media (max-width: 720px) {
  .basic-grid {
    grid-template-columns: 1fr;
  }
}
</style>
