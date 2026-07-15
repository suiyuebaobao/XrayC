<!--
  本组件展示资源健康、探测故障转移策略和计费概况。
  策略表单接收父级 settingsForm，并通过 save 事件交给 composable 保存。
  资源和计费数字来自运营 summary，不在组件内重新请求接口。
  该组件集中承载原页面底部的运营状态卡片。
-->
<script setup lang="ts">
import type { AccessOperationsSettings, OperationsSummary } from '@/services/api';
import { formatKnownNumber, formatTraffic } from '@/views/operations/operationsFormatters';

defineProps<{
  savingSettings: boolean;
  settingsError: string;
  settingsForm: AccessOperationsSettings;
  settingsMessage: string;
  summary: OperationsSummary | undefined;
}>();

const emit = defineEmits<{
  (event: 'save'): void;
}>();
</script>

<template>
  <el-row :gutter="18" class="section-row">
    <el-col :xs="24" :lg="12">
      <el-card shadow="never" class="operations-card">
        <template #header>资源健康</template>
        <div class="operations-list">
          <div>
            <span>启用线路</span>
            <strong>{{ formatKnownNumber(summary?.activeAccessLines) }}</strong>
          </div>
          <div>
            <span>可用线路集合</span>
            <strong>{{ formatKnownNumber(summary?.healthyExitPools) }}</strong>
          </div>
        </div>
      </el-card>
    </el-col>

    <el-col :xs="24" :lg="12">
      <el-card shadow="never" class="operations-card">
        <template #header>
          <div class="operations-card__header">
            <span>探测故障转移策略</span>
            <el-tag effect="plain" :type="settingsForm.probePolicy.exitAutoFailoverEnabled ? 'success' : 'info'">
              {{ settingsForm.probePolicy.exitAutoFailoverEnabled ? '自动生效' : '仅记录' }}
            </el-tag>
          </div>
        </template>

        <el-alert
          v-if="settingsError"
          class="settings-alert"
          :title="settingsError"
          type="warning"
          show-icon
          :closable="false"
        />
        <el-alert
          v-else-if="settingsMessage"
          class="settings-alert"
          :title="settingsMessage"
          type="success"
          show-icon
          :closable="false"
        />

        <el-form label-position="top" class="probe-settings">
          <el-form-item label="启用自动故障转移">
            <el-switch v-model="settingsForm.probePolicy.exitAutoFailoverEnabled" />
          </el-form-item>
          <el-form-item label="连续失败阈值">
            <el-input-number
              v-model="settingsForm.probePolicy.exitFailureThreshold"
              :min="1"
              :max="20"
              controls-position="right"
            />
          </el-form-item>
          <el-form-item label="连续恢复阈值">
            <el-input-number
              v-model="settingsForm.probePolicy.exitRecoveryThreshold"
              :min="1"
              :max="20"
              controls-position="right"
            />
          </el-form-item>
          <el-form-item label="探测窗口（分钟）">
            <el-input-number
              v-model="settingsForm.probePolicy.exitWindowMinutes"
              :min="1"
              :max="120"
              controls-position="right"
            />
          </el-form-item>
          <el-form-item label="定时探测间隔（秒）">
            <el-input-number
              v-model="settingsForm.probePolicy.exitProbeIntervalSeconds"
              :min="30"
              :max="86400"
              controls-position="right"
            />
          </el-form-item>
          <el-form-item label="每轮排队上限">
            <el-input-number
              v-model="settingsForm.probePolicy.probeQueueBatchSize"
              :min="1"
              :max="1000"
              controls-position="right"
            />
          </el-form-item>
          <el-form-item label="单节点积压上限">
            <el-input-number
              v-model="settingsForm.probePolicy.maxPendingProbeTasks"
              :min="1"
              :max="100"
              controls-position="right"
            />
          </el-form-item>
          <el-form-item label="详细流量日志保留天数">
            <el-input-number
              v-model="settingsForm.trafficLogRetention.detailRetentionDays"
              :min="1"
              :max="3650"
              controls-position="right"
            />
          </el-form-item>
          <el-form-item label="启用自动数据库备份">
            <el-switch v-model="settingsForm.databaseBackup.enabled" />
          </el-form-item>
          <el-form-item label="数据库备份间隔（天）">
            <el-input-number
              v-model="settingsForm.databaseBackup.intervalDays"
              :min="1"
              :max="30"
              controls-position="right"
              :disabled="!settingsForm.databaseBackup.enabled"
            />
          </el-form-item>
          <el-form-item label="数据库备份保留天数">
            <el-input-number
              v-model="settingsForm.databaseBackup.retentionDays"
              :min="1"
              :max="3650"
              controls-position="right"
              :disabled="!settingsForm.databaseBackup.enabled"
            />
          </el-form-item>
        </el-form>
        <p class="probe-settings__hint">
          探测策略按“中转节点到出口”的组合健康生效；详细流量日志超过保留天数后会先汇总再删除；数据库备份默认写入服务器 /backups 目录。
        </p>
        <el-button type="primary" :loading="savingSettings" @click="emit('save')">保存设置</el-button>
      </el-card>
    </el-col>
  </el-row>

  <el-row :gutter="18" class="section-row">
    <el-col :xs="24" :lg="12">
      <el-card shadow="never" class="operations-card">
        <template #header>计费概况</template>
        <div class="operations-list">
          <div>
            <span>本月扣费流量</span>
            <strong>{{ formatTraffic(summary?.monthlyBilledTrafficGb) }}</strong>
          </div>
          <div>
            <span>账本记录</span>
            <strong>{{ formatKnownNumber(summary?.ledgerCount) }}</strong>
          </div>
        </div>
      </el-card>
    </el-col>
  </el-row>
</template>
