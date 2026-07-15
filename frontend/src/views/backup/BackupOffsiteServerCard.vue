<!--
  页面顶部「异地备份服务器」共享连接卡：异地是共享 rsync 目标、不再是独立备份模式，
  全量/WAL 各自的「存储位置」选到异地时就推到这台机。
  维护 SSH 连接（IP/端口/用户/密码/远程目录）+ 异地保留天数 + 「测试并装公钥」+ 公钥状态/指纹。
  SSH 密码写值不回显（占位由 sshPasswordSet 决定，留空=不改）；provision 交父级处理。
  config 是父级 reactive 异地配置对象，直接就地改写（是否推异地由全量/WAL 的 destination 决定）。
-->
<script setup lang="ts">
import { Key } from '@element-plus/icons-vue';
import type { BackupOffsiteConfig } from '@/services/api';
import { secretPlaceholder } from './backupSchedule';

defineProps<{
  config: BackupOffsiteConfig;
  testing: boolean;
}>();

const emit = defineEmits<{
  (event: 'test'): void;
}>();
</script>

<template>
  <el-card shadow="never" class="server-card">
    <template #header>
      <div class="server-head">
        <span>异地备份服务器（rsync 共享目标 · 全量/WAL 选「异地」时推这里）</span>
        <div class="server-head__status">
          <el-tag v-if="config.pubkeyInstalled" type="success" effect="plain">公钥已装</el-tag>
          <el-tag v-else type="info" effect="plain">公钥未装</el-tag>
        </div>
      </div>
    </template>

    <el-form label-position="top" class="server-form">
      <el-row :gutter="18">
        <el-col :xs="24" :md="8">
          <el-form-item label="服务器 IP">
            <el-input v-model="config.sshHost" placeholder="必须为 IP（域名/CF 无法直连 SSH）" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="8">
          <el-form-item label="SSH 端口">
            <el-input-number v-model="config.sshPort" :min="1" :max="65535" controls-position="right" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="8">
          <el-form-item label="SSH 用户">
            <el-input v-model="config.sshUser" placeholder="默认 root" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="8">
          <el-form-item label="SSH 密码">
            <el-input
              v-model="config.sshPassword"
              type="password"
              show-password
              :placeholder="secretPlaceholder(config.sshPasswordSet)"
            />
            <small class="field-hint">仅作密钥失效兜底，日常同步走密钥免密；留空保留原值。</small>
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="8">
          <el-form-item label="远程目录">
            <el-input v-model="config.remoteDir" placeholder="/var/backups/xrayc" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="8">
          <el-form-item label="异地保留天数">
            <el-input-number v-model="config.retentionDays" :min="1" :max="3650" controls-position="right" />
            <small class="field-hint">异地机上保留最近多少天的备份文件（全量/WAL 共用）。</small>
          </el-form-item>
        </el-col>
      </el-row>

      <div class="provision">
        <el-button type="primary" plain :icon="Key" :loading="testing" @click="emit('test')">
          测试并装公钥
        </el-button>
        <span v-if="config.pubkeyFingerprint" class="provision__fp">
          指纹：{{ config.pubkeyFingerprint }}
        </span>
      </div>
      <small class="field-hint">
        点击后系统用上面的密码 SSH 进异地机装中心机公钥并验证免密；成功后置「公钥已装」。
      </small>
    </el-form>
  </el-card>
</template>

<style scoped>
.server-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.server-form {
  max-width: 100%;
}

.provision {
  display: flex;
  align-items: center;
  gap: 14px;
  flex-wrap: wrap;
  margin-bottom: 6px;
}

.provision__fp {
  color: var(--el-text-color-secondary);
  font-size: 13px;
  word-break: break-all;
}

.field-hint {
  display: block;
  margin-top: 4px;
  color: var(--el-text-color-secondary);
}
</style>
