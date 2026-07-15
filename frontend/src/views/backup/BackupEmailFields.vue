<!--
  邮件发文件「编辑」弹窗的表单字段片段（不含卡片/启用开关，供 BackupEditDialog 内嵌）。
  含收件人（多个）、加密附件开关、附件口令（脱敏，留空=不改）、单封上限 MB、超限处理（仅通知/自动分片）、成功/失败通知。
  附件口令写值不回显（占位由 attachPassphraseSet 决定）；config 直接就地改写（同其它字段片段口径）。
-->
<script setup lang="ts">
import { computed } from 'vue';
import type { BackupEmailConfig } from '@/services/api';
import BackupScheduleFields from './BackupScheduleFields.vue';
import { formatRecipients, parseRecipients, secretPlaceholder } from './backupSchedule';

const props = defineProps<{
  config: BackupEmailConfig;
}>();

// 收件人数组 <-> 多行文本框：读格式化、写解析去重（纯函数）。
const recipientsText = computed({
  get: () => formatRecipients(props.config.recipients),
  set: (value: string) => {
    props.config.recipients = parseRecipients(value);
  },
});
</script>

<template>
  <el-form label-position="top" class="fields-form">
    <el-form-item label="收件人（每行一个）">
      <el-input
        v-model="recipientsText"
        type="textarea"
        :rows="3"
        placeholder="ops@example.com&#10;backup@example.com"
      />
      <small class="field-hint">可填多个，换行/逗号/分号分隔，自动去重。</small>
    </el-form-item>
    <BackupScheduleFields :schedule="config.schedule" />
    <el-form-item label="加密附件">
      <el-switch v-model="config.attachEncrypted" active-text="加密" inactive-text="明文" />
      <small class="field-hint">用 AES-256 加密后作附件；口令用于解密恢复。</small>
    </el-form-item>
    <el-form-item label="附件口令">
      <el-input
        v-model="config.attachPassphrase"
        type="password"
        show-password
        :disabled="!config.attachEncrypted"
        :placeholder="secretPlaceholder(config.attachPassphraseSet)"
      />
      <small class="field-hint">写值不回显；留空保留原值。</small>
    </el-form-item>
    <el-form-item label="单封附件上限（MB）">
      <el-input-number v-model="config.maxAttachMb" :min="1" :max="50" controls-position="right" />
      <small class="field-hint">附件超过此上限时按下面「附件超过上限时」处理，不静默。</small>
    </el-form-item>
    <el-form-item label="附件超过上限时">
      <el-radio-group v-model="config.oversizeMode">
        <el-radio-button value="notify">仅通知（推荐）</el-radio-button>
        <el-radio-button value="split">自动分片发送</el-radio-button>
      </el-radio-group>
      <small v-if="config.oversizeMode === 'split'" class="field-hint">会拆成多封邮件、需按说明拼回。</small>
      <small v-else class="field-hint">超限时只发一封通知、不带附件，避免整封失败。</small>
    </el-form-item>
    <el-form-item label="通知">
      <div class="notify-switches">
        <el-switch v-model="config.notifyOnSuccess" active-text="成功通知" inactive-text="" />
        <el-switch v-model="config.notifyOnFailure" active-text="失败通知" inactive-text="" />
      </div>
    </el-form-item>
  </el-form>
</template>

<style scoped>
.fields-form {
  max-width: 100%;
}

.notify-switches {
  display: flex;
  gap: 20px;
  flex-wrap: wrap;
}

.field-hint {
  display: block;
  margin-top: 4px;
  color: var(--el-text-color-secondary);
}
</style>
