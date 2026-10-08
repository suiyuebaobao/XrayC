<!-- 原有弹窗入口保留，表单与新增节点精灵共用。 -->
<script setup lang="ts">
import AccessNodeForm from './AccessNodeForm.vue';
import type { NodeForm } from '@/views/access-lines/types';
const visible = defineModel<boolean>({ required: true });
const form = defineModel<NodeForm>('form', { required: true });
defineProps<{
  mode: 'create' | 'edit';
  saving: boolean;
}>();
const emit = defineEmits<{ (event: 'save'): void; }>();
</script>
<template>
  <el-dialog v-model="visible" :title="mode === 'edit' ? '编辑中转节点' : '新增中转节点'" width="860px">
    <AccessNodeForm v-model:form="form" :mode="mode" :saving="saving" @save="emit('save')" @cancel="visible = false" />
  </el-dialog>
</template>
