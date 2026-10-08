<!-- 原有弹窗入口保留，表单与新增节点精灵共用。 -->
<script setup lang="ts">
import OneClickAgentInstallForm from './OneClickAgentInstallForm.vue';
import type { OneClickInstallForm } from '@/views/access-lines/types';
const visible = defineModel<boolean>({ required: true });
const form = defineModel<OneClickInstallForm>('form', { required: true });
defineProps<{
  saving: boolean;
}>();
const emit = defineEmits<{ (event: 'submit'): void; }>();
</script>
<template>
  <el-dialog v-model="visible" title="一键安装 Agent" width="860px">
    <OneClickAgentInstallForm v-model:form="form" :saving="saving" @submit="emit('submit')" @cancel="visible = false" />
  </el-dialog>
</template>
