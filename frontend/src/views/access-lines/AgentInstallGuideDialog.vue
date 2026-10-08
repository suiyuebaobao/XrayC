<!-- 原有弹窗入口保留，表单与新增节点精灵共用。 -->
<script setup lang="ts">
import AgentInstallGuideContent from './AgentInstallGuideContent.vue';
import type { InstallGuideView } from '@/views/access-lines/types';
const visible = defineModel<boolean>({ required: true });
defineProps<{
  saving: boolean;
  installGuide: InstallGuideView | null;
}>();
const emit = defineEmits<{ (event: 'generate'): void; }>();
</script>
<template>
  <el-dialog v-model="visible" title="Agent 安装说明" width="860px">
    <AgentInstallGuideContent :install-guide="installGuide" :saving="saving" @generate="emit('generate')" @cancel="visible = false" />
  </el-dialog>
</template>
