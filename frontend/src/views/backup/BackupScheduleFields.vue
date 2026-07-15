<!--
  数据库备份「调度」通用子块，被全量 / 异地 / WAL 三处复用（守 550 行上限）。
  面向不懂 cron 的用户：只提供纯中文的四档选择器（按间隔 / 每天 / 每周 / 每月），
  界面绝不出现 cron 字样、也绝不让用户填 cron 表达式；所有生成/解析交给 backupSchedule.ts 的纯函数。
  schedule 是父级 reactive 调度对象，用户改动任意选项后就地写回（未改动不写，避免覆盖历史遗留的高级 cron）。
-->
<script setup lang="ts">
import { computed, reactive, watch } from 'vue';
import type { BackupSchedule } from '@/services/api';
import {
  isAdvancedCron,
  scheduleFromForm,
  scheduleToForm,
  type IntervalUnit,
  type ScheduleForm,
  type ScheduleMode,
} from './backupSchedule';

const props = defineProps<{
  schedule: BackupSchedule;
  disabled?: boolean;
}>();

// 本地表单态：把后端 schedule 翻译成纯中文选择项，用户只操作 form。
const form = reactive<ScheduleForm>(scheduleToForm(props.schedule));

// schedule 换了对象（如编辑弹窗每次打开都重建草稿）时，重新翻译回填 form。
watch(
  () => props.schedule,
  (next) => {
    Object.assign(form, scheduleToForm(next));
  },
);

// 命中历史遗留 / 复杂定时规则时给只读提示：保留原设置，改动下方选项才会覆盖。
const advanced = computed(() => isAdvancedCron(props.schedule));

// 用户改动任意选项后，把 form 翻译回 schedule 就地写回父级草稿。
function apply() {
  const next = scheduleFromForm(form.mode, form);
  props.schedule.kind = next.kind;
  props.schedule.intervalMinutes = next.intervalMinutes;
  props.schedule.cron = next.cron;
}

const MODE_OPTIONS: { value: ScheduleMode; label: string }[] = [
  { value: 'interval', label: '按间隔' },
  { value: 'daily', label: '每天' },
  { value: 'weekly', label: '每周' },
  { value: 'monthly', label: '每月' },
];

const UNIT_OPTIONS: { value: IntervalUnit; label: string }[] = [
  { value: 'minute', label: '分钟' },
  { value: 'hour', label: '小时' },
];

// 周日=0 … 周六=6，与后端周字段一致。
const WEEKDAY_OPTIONS = [
  { value: 0, label: '周日' },
  { value: 1, label: '周一' },
  { value: 2, label: '周二' },
  { value: 3, label: '周三' },
  { value: 4, label: '周四' },
  { value: 5, label: '周五' },
  { value: 6, label: '周六' },
];

// 每月 1…28 号（避开 29-31 在小月缺失）。
const MONTHDAY_OPTIONS = Array.from({ length: 28 }, (_, index) => index + 1);
</script>

<template>
  <el-form-item label="调度方式">
    <el-select
      v-model="form.mode"
      :disabled="disabled"
      class="mode-select"
      @change="apply"
    >
      <el-option v-for="opt in MODE_OPTIONS" :key="opt.value" :value="opt.value" :label="opt.label" />
    </el-select>
    <small v-if="advanced" class="field-hint">
      检测到高级定时规则，已保留原设置；改动下方选项将覆盖为所选方式。
    </small>
  </el-form-item>

  <!-- 按间隔：每 N 分钟 / 小时 -->
  <el-form-item v-if="form.mode === 'interval'" label="执行频率">
    <span class="inline-label">每</span>
    <el-input-number
      v-model="form.intervalNum"
      :min="1"
      :max="100000"
      :disabled="disabled"
      controls-position="right"
      @change="apply"
    />
    <el-select v-model="form.intervalUnit" :disabled="disabled" class="unit-select" @change="apply">
      <el-option v-for="opt in UNIT_OPTIONS" :key="opt.value" :value="opt.value" :label="opt.label" />
    </el-select>
    <small class="field-hint">worker 每 60 秒巡检一次，支持分钟级。</small>
  </el-form-item>

  <!-- 每天：每天 HH:mm -->
  <el-form-item v-else-if="form.mode === 'daily'" label="执行时间">
    <span class="inline-label">每天</span>
    <el-time-picker
      v-model="form.time"
      value-format="HH:mm"
      format="HH:mm"
      :clearable="false"
      :disabled="disabled"
      placeholder="选择时间"
      @change="apply"
    />
  </el-form-item>

  <!-- 每周：每周 周X HH:mm -->
  <el-form-item v-else-if="form.mode === 'weekly'" label="执行时间">
    <span class="inline-label">每</span>
    <el-select v-model="form.weekday" :disabled="disabled" class="day-select" @change="apply">
      <el-option v-for="opt in WEEKDAY_OPTIONS" :key="opt.value" :value="opt.value" :label="opt.label" />
    </el-select>
    <el-time-picker
      v-model="form.time"
      value-format="HH:mm"
      format="HH:mm"
      :clearable="false"
      :disabled="disabled"
      placeholder="选择时间"
      @change="apply"
    />
  </el-form-item>

  <!-- 每月：每月 N 号 HH:mm -->
  <el-form-item v-else label="执行时间">
    <span class="inline-label">每月</span>
    <el-select v-model="form.monthday" :disabled="disabled" class="day-select" @change="apply">
      <el-option v-for="day in MONTHDAY_OPTIONS" :key="day" :value="day" :label="`${day} 号`" />
    </el-select>
    <el-time-picker
      v-model="form.time"
      value-format="HH:mm"
      format="HH:mm"
      :clearable="false"
      :disabled="disabled"
      placeholder="选择时间"
      @change="apply"
    />
  </el-form-item>
</template>

<style scoped>
.mode-select {
  width: 140px;
}

.unit-select {
  width: 96px;
  margin-left: 8px;
}

.day-select {
  width: 104px;
  margin-right: 8px;
}

.inline-label {
  margin-right: 8px;
  color: var(--el-text-color-regular);
}

.field-hint {
  display: block;
  margin-top: 4px;
  color: var(--el-text-color-secondary);
}
</style>
