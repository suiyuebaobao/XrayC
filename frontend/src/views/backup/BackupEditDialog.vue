<!--
  备份「编辑」弹窗：每种模式复用一个弹窗，按 mode 渲染对应字段片段（全量/WAL/邮件）。
  弹窗操作的是父级传入的 draft（config 的深拷贝副本）：保存时父级用它调 updateBackupConfig，
  取消/关闭直接丢弃、不动线上 config，故密码/口令留空=不改、且取消不会污染主表状态。
-->
<script setup lang="ts">
import { computed } from 'vue';
import type { BackupActiveMode, BackupConfig } from '@/services/api';
import { runModeLabel } from './backupSchedule';
import BackupEmailFields from './BackupEmailFields.vue';
import BackupFullFields from './BackupFullFields.vue';
import BackupWalFields from './BackupWalFields.vue';

const props = defineProps<{
  modelValue: boolean;
  mode: BackupActiveMode | null;
  // 编辑草稿（config 深拷贝）；父级懒初始化，未就绪时为 null，弹窗内做空值守卫。
  draft: BackupConfig | null;
  saving: boolean;
}>();

const emit = defineEmits<{
  (event: 'update:modelValue', value: boolean): void;
  (event: 'save'): void;
}>();

const title = computed(() => (props.mode ? `编辑 · ${runModeLabel(props.mode)}` : '编辑'));

function close() {
  emit('update:modelValue', false);
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    :title="title"
    width="640px"
    :close-on-click-modal="false"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <template v-if="draft">
      <BackupFullFields v-if="mode === 'full'" :config="draft.full" />
      <BackupWalFields v-else-if="mode === 'wal'" :config="draft.wal" />
      <BackupEmailFields v-else-if="mode === 'email'" :config="draft.email" />
    </template>

    <template #footer>
      <el-button @click="close">取消</el-button>
      <el-button type="primary" :loading="saving" @click="emit('save')">保存</el-button>
    </template>
  </el-dialog>
</template>
