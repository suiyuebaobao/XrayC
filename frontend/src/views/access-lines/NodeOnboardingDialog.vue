<!-- 三种接入方式共用一个精灵，调用原有 API 与协议校验，不重复维护业务逻辑。 -->
<script setup lang="ts">
import { computed } from 'vue';
import AccessNodeForm from './AccessNodeForm.vue';
import OneClickAgentInstallForm from './OneClickAgentInstallForm.vue';
import AgentInstallGuideContent from './AgentInstallGuideContent.vue';
import type { InstallGuideView, NodeForm, OneClickInstallForm } from './types';

const visible = defineModel<boolean>({ required: true });
const nodeForm = defineModel<NodeForm>('nodeForm', { required: true });
const installForm = defineModel<OneClickInstallForm>('installForm', { required: true });
const props = defineProps<{
  method: 'choose' | 'automatic' | 'manual' | 'existing';
  saving: boolean;
  installGuide: InstallGuideView | null;
}>();
const emit = defineEmits<{
  choose: [method: 'choose' | 'automatic' | 'manual' | 'existing'];
  install: [];
  generate: [];
  save: [];
}>();
const methods = [
  { id: 'automatic' as const, title: '一键安装 Agent', description: '提供 SSH 连接资料，系统预检、安装并登记节点。', step: '配置服务器' },
  { id: 'manual' as const, title: '手动安装 Agent', description: '取得安装命令，在服务器执行后填入鉴权码。', step: '服务器安装' },
  { id: 'existing' as const, title: '接入已有 Agent', description: 'Agent 已安装，使用节点鉴权码完成接入。', step: '填写接入资料' },
];
const selected = computed(() => methods.find((item) => item.id === props.method));
const title = computed(() => selected.value?.title ?? '新增中转节点');
</script>

<template>
  <el-dialog v-model="visible" :title="title" width="860px" :close-on-click-modal="!saving" :close-on-press-escape="!saving" :show-close="!saving" destroy-on-close>
    <el-steps :active="method === 'choose' ? 0 : 1" simple class="onboarding-steps">
      <el-step title="选择方式" />
      <el-step :title="selected?.step ?? '配置接入'" />
      <el-step title="确认节点状态" />
    </el-steps>
    <div v-if="method === 'choose'" class="onboarding-methods">
      <button v-for="(item, index) in methods" :key="item.id" type="button" :aria-label="item.title" class="onboarding-method" @click="emit('choose', item.id)">
        <span class="onboarding-number">0{{ index + 1 }}</span>
        <strong>{{ item.title }}</strong>
        <span>{{ item.description }}</span>
      </button>
    </div>
    <template v-else>
      <div class="onboarding-context">
        <span>{{ selected?.description }}</span>
        <el-button link :disabled="saving" @click="emit('choose', 'choose')">更换方式</el-button>
      </div>
      <OneClickAgentInstallForm v-if="method === 'automatic'" v-model:form="installForm" :saving="saving" @submit="emit('install')" @cancel="visible = false" />
      <AgentInstallGuideContent v-else-if="method === 'manual'" :saving="saving" :install-guide="installGuide" @generate="emit('generate')" @cancel="visible = false" />
      <AccessNodeForm v-else v-model:form="nodeForm" mode="create" :saving="saving" @save="emit('save')" @cancel="visible = false" />
      <el-button v-if="method === 'manual' && installGuide" class="onboarding-next" type="primary" plain @click="emit('choose', 'existing')">已完成安装，继续接入</el-button>
      <p class="onboarding-hint">提交后可在节点页查看部署进度、Agent 心跳及配置同步状态。</p>
    </template>
    <template v-if="method === 'choose'" #footer>
      <el-button @click="visible = false">取消</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.onboarding-steps { margin-bottom: 24px; }
.onboarding-methods { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 14px; }
.onboarding-method { display: flex; flex-direction: column; text-align: left; gap: 14px; padding: 22px; border: 1px solid var(--border); border-radius: 12px; background: var(--surface); color: var(--ink-soft); cursor: pointer; font: inherit; line-height: 1.65; }
.onboarding-method:hover, .onboarding-method:focus-visible { border-color: var(--el-color-primary); outline: 2px solid var(--el-color-primary-light-8); }
.onboarding-method strong { color: var(--ink); font-size: 16px; }
.onboarding-number { font-weight: 700; color: var(--el-color-primary); }
.onboarding-context { display: flex; justify-content: space-between; align-items: start; gap: 16px; margin-bottom: 20px; color: var(--ink-soft); }
.onboarding-next { margin-top: 18px; }
.onboarding-hint { margin: 18px 0 0; color: var(--ink-soft); font-size: 12px; }
@media (max-width: 640px) { .onboarding-methods { grid-template-columns: 1fr; } .onboarding-method { gap: 6px; padding: 16px; } .onboarding-steps { padding: 12px 8px; } }
</style>
