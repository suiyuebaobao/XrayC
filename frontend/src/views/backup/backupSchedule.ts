// 中文说明：数据库备份页的纯逻辑工具（可独立单测，无 DOM/网络依赖）。
// 覆盖：脱敏占位文案、调度 kind 切换保值、间隔/cron 校验、收件人解析、状态与模式展示映射。
// 页面组件只做绑定与展示，判定逻辑集中在此，便于 vitest 覆盖边界。
import type { BackupRunMode, BackupSchedule, BackupScheduleKind } from '@/services/api';

// 敏感字段占位文案：由后端返回的 `*_set` 布尔决定，绝不展示明文。
export const SECRET_SET_PLACEHOLDER = '已设置（留空则不修改）';
export const SECRET_UNSET_PLACEHOLDER = '未设置（请输入）';

export function secretPlaceholder(isSet: boolean): string {
  return isSet ? SECRET_SET_PLACEHOLDER : SECRET_UNSET_PLACEHOLDER;
}

// 间隔分钟数归一：非有限值或 <1 一律回退到 1（输入下限）。
export function sanitizeIntervalMinutes(value: number): number {
  if (!Number.isFinite(value) || value < 1) {
    return 1;
  }
  return Math.floor(value);
}

// 切换调度类型时保留另一种类型已填的值，避免来回切丢失配置；切到间隔时顺带归一。
export function applyScheduleKind(schedule: BackupSchedule, kind: BackupScheduleKind): BackupSchedule {
  return {
    kind,
    intervalMinutes: kind === 'interval'
      ? sanitizeIntervalMinutes(schedule.intervalMinutes)
      : schedule.intervalMinutes,
    cron: schedule.cron,
  };
}

// cron 只做轻量结构校验：必须恰好 5 段（分 时 日 月 周）。
export function isCronExpressionValid(cron: string): boolean {
  const fields = cron.trim().split(/\s+/).filter(Boolean);
  return fields.length === 5;
}

// 调度整体是否有效：间隔需 ≥1，cron 需 5 段。
export function isScheduleValid(schedule: BackupSchedule): boolean {
  if (schedule.kind === 'cron') {
    return isCronExpressionValid(schedule.cron);
  }
  return Number.isFinite(schedule.intervalMinutes) && schedule.intervalMinutes >= 1;
}

// 人类可读的调度摘要，供状态/说明区展示。
export function describeSchedule(schedule: BackupSchedule): string {
  if (schedule.kind === 'cron') {
    const cron = schedule.cron.trim();
    return cron ? `Cron：${cron}` : 'Cron：未设置';
  }
  return `每 ${sanitizeIntervalMinutes(schedule.intervalMinutes)} 分钟`;
}

// ── 纯中文调度选择器：把面向用户的四档中文选项与后端 schedule（间隔分钟/cron）互相翻译 ──
// 界面只让用户选「按间隔 / 每天 / 每周 / 每月」，绝不暴露 cron；生成/解析都在这里做，页面只绑定。

// 面向用户的四档调度方式。
export type ScheduleMode = 'interval' | 'daily' | 'weekly' | 'monthly';
// 间隔单位：分钟或小时（小时 = 60 分钟）。
export type IntervalUnit = 'minute' | 'hour';

// 表单态：所有档位共用一份，切档不丢已填值（跨档保留时间/周几/号/间隔）。
export type ScheduleForm = {
  mode: ScheduleMode;
  // 按间隔：每 intervalNum 个 intervalUnit（分钟/小时）。
  intervalNum: number;
  intervalUnit: IntervalUnit;
  // 每天/每周/每月共用的执行时间，格式 "HH:mm"。
  time: string;
  // 每周的星期：周日=0 … 周六=6（与 cron 周字段一致）。
  weekday: number;
  // 每月的号：1…28（避开 29-31 在小月缺失的问题）。
  monthday: number;
};

// 两位补零，仅用于生成展示用 "HH:mm"（cron 本身用不补零的数字）。
function pad2(value: number): string {
  return value < 10 ? `0${value}` : String(value);
}

// 纯数字字段解析：非纯数字返回 null（用于把 cron 复杂表达式判为「高级」走兜底）。
function parseIntField(field: string): number | null {
  return /^\d+$/.test(field) ? Number(field) : null;
}

// 归一星期到 0-6，非法回退周一(1)。
function clampWeekday(value: number): number {
  return Number.isInteger(value) && value >= 0 && value <= 6 ? value : 1;
}

// 归一每月号到 1-28，非法回退 1 号。
function clampMonthday(value: number): number {
  return Number.isInteger(value) && value >= 1 && value <= 28 ? value : 1;
}

// 把 "HH:mm" 解析成时/分，缺失或非法回退默认 03:00。
function hmFromTime(time: string): { hour: number; minute: number } {
  const matched = /^(\d{1,2}):(\d{1,2})$/.exec((time ?? '').trim());
  let hour = matched ? Number(matched[1]) : 3;
  let minute = matched ? Number(matched[2]) : 0;
  if (!Number.isInteger(hour) || hour < 0 || hour > 23) {
    hour = 3;
  }
  if (!Number.isInteger(minute) || minute < 0 || minute > 59) {
    minute = 0;
  }
  return { hour, minute };
}

// 时/分拼成展示用 "HH:mm"（补零，供 el-time-picker value-format）。
function timeFromHM(hour: number, minute: number): string {
  return `${pad2(hour)}:${pad2(minute)}`;
}

// 间隔分钟数 → 数字+单位：能整除 60 显示「小时」，否则「分钟」。
function intervalToNumUnit(minutes: number): { intervalNum: number; intervalUnit: IntervalUnit } {
  const total = sanitizeIntervalMinutes(minutes);
  if (total % 60 === 0) {
    return { intervalNum: total / 60, intervalUnit: 'hour' };
  }
  return { intervalNum: total, intervalUnit: 'minute' };
}

// 解析「简单」cron，仅认三种模式，命中返回结构化字段，否则 null（判为高级/历史遗留）：
//   "m h * * *"     → 每天
//   "m h * * w"     → 每周（w 为单个 0-6）
//   "m h d * *"     → 每月（d 为单个 1-28）
function parseSimpleCron(
  cron: string,
): { mode: Exclude<ScheduleMode, 'interval'>; hour: number; minute: number; weekday: number; monthday: number } | null {
  const fields = cron.trim().split(/\s+/).filter(Boolean);
  if (fields.length !== 5) {
    return null;
  }
  const [minuteField, hourField, domField, monField, dowField] = fields;
  const minute = parseIntField(minuteField);
  const hour = parseIntField(hourField);
  // 分/时必须是合法单值；月份必须通配（我们只支持每天/每周/每月三种简单节律）。
  if (minute === null || minute < 0 || minute > 59) {
    return null;
  }
  if (hour === null || hour < 0 || hour > 23) {
    return null;
  }
  if (monField !== '*') {
    return null;
  }
  // 每天：日、周都通配。
  if (domField === '*' && dowField === '*') {
    return { mode: 'daily', hour, minute, weekday: 1, monthday: 1 };
  }
  // 每周：日通配、周为单个 0-6。
  if (domField === '*' && dowField !== '*') {
    const weekday = parseIntField(dowField);
    if (weekday === null || weekday < 0 || weekday > 6) {
      return null;
    }
    return { mode: 'weekly', hour, minute, weekday, monthday: 1 };
  }
  // 每月：周通配、日为单个 1-28。
  if (dowField === '*' && domField !== '*') {
    const monthday = parseIntField(domField);
    if (monthday === null || monthday < 1 || monthday > 28) {
      return null;
    }
    return { mode: 'monthly', hour, minute, weekday: 1, monthday };
  }
  return null;
}

// 表单 → 后端 schedule：四档各生成对应 kind/间隔/cron。
// intervalMinutes 在所有档位都按「间隔数字×单位」算出并带上，使跨档切换时间隔设置不丢
// （cron 档后端只认 cron、忽略该值，但保留它可让用户切回「按间隔」时看到原来的间隔）。
export function scheduleFromForm(mode: ScheduleMode, form: ScheduleForm): BackupSchedule {
  const intervalMinutes = sanitizeIntervalMinutes(form.intervalNum) * (form.intervalUnit === 'hour' ? 60 : 1);
  if (mode === 'interval') {
    return { kind: 'interval', intervalMinutes, cron: '' };
  }
  const { hour, minute } = hmFromTime(form.time);
  if (mode === 'weekly') {
    return { kind: 'cron', intervalMinutes, cron: `${minute} ${hour} * * ${clampWeekday(form.weekday)}` };
  }
  if (mode === 'monthly') {
    return { kind: 'cron', intervalMinutes, cron: `${minute} ${hour} ${clampMonthday(form.monthday)} * *` };
  }
  return { kind: 'cron', intervalMinutes, cron: `${minute} ${hour} * * *` };
}

// 后端 schedule → 表单（回显）：
//   interval → 按间隔（能整除 60 显示小时否则分钟）；
//   简单 cron → 每天/每周/每月 + 时间；
//   复杂/历史 cron（不匹配三种简单模式）→ 兜底回退「按间隔」，默认每天=1440 分钟=24 小时，绝不崩。
export function scheduleToForm(schedule: BackupSchedule): ScheduleForm {
  const { intervalNum, intervalUnit } = intervalToNumUnit(schedule.intervalMinutes);
  const base: ScheduleForm = {
    mode: 'daily',
    intervalNum,
    intervalUnit,
    time: '03:00',
    weekday: 1,
    monthday: 1,
  };
  if (schedule.kind === 'interval') {
    return { ...base, mode: 'interval' };
  }
  const parsed = parseSimpleCron(schedule.cron);
  if (!parsed) {
    // 兜底：复杂/历史 cron 无法用中文选择器表达，回退成按间隔（每天=1440 分钟=24 小时）。
    return { ...base, mode: 'interval', intervalNum: 24, intervalUnit: 'hour' };
  }
  return {
    ...base,
    mode: parsed.mode,
    time: timeFromHM(parsed.hour, parsed.minute),
    weekday: parsed.weekday,
    monthday: parsed.monthday,
  };
}

// 是否为「高级/历史遗留」cron（不匹配三种简单模式）：供 UI 给只读提示、保留原设置不覆盖。
export function isAdvancedCron(schedule: BackupSchedule): boolean {
  return schedule.kind === 'cron' && parseSimpleCron(schedule.cron) === null;
}

// 收件人解析：按换行/逗号/分号/空白切分，trim、去空、去重，保持输入顺序。
export function parseRecipients(text: string): string[] {
  const seen = new Set<string>();
  const list: string[] = [];
  for (const raw of text.split(/[\s,;]+/)) {
    const item = raw.trim();
    if (!item || seen.has(item)) {
      continue;
    }
    seen.add(item);
    list.push(item);
  }
  return list;
}

// 收件人数组回填到多行文本框（一行一个）。
export function formatRecipients(recipients: string[]): string {
  return recipients.join('\n');
}

const RUN_MODE_LABELS: Record<BackupRunMode, string> = {
  full: '全量备份',
  offsite: '异地同步',
  wal: 'WAL 基准备份',
  email: '邮件发送',
};

export function runModeLabel(mode: BackupRunMode): string {
  return RUN_MODE_LABELS[mode] ?? mode;
}

export function backupStatusLabel(status: string): string {
  switch (status) {
    case 'success':
      return '成功';
    case 'running':
      return '进行中';
    case 'queued':
      return '排队中';
    case 'failed':
      return '失败';
    case '':
    case 'idle':
      return '未运行';
    default:
      return status;
  }
}

export type BackupStatusTagType = 'success' | 'info' | 'warning' | 'danger';

export function backupStatusTagType(status: string): BackupStatusTagType {
  switch (status) {
    case 'success':
      return 'success';
    case 'running':
    case 'queued':
      return 'warning';
    case 'failed':
      return 'danger';
    default:
      return 'info';
  }
}

// 时间展示：空串显示占位破折号，非法时间原样返回，否则本地化到秒。
export function formatBackupTime(value: string): string {
  if (!value) {
    return '—';
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString('zh-CN', { hour12: false });
}

// 字节数人类可读：B/KB/MB/GB（封顶 GB），KB 起保留 1 位小数、B 取整；
// 非有限值或负数按 0 处理（备份历史里失败/无产物条目 size 为 0）。
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) {
    return '0 B';
  }
  if (bytes < 1024) {
    return `${Math.round(bytes)} B`;
  }
  const units = ['KB', 'MB', 'GB'];
  let scaled = bytes;
  let unitIndex = -1;
  do {
    scaled /= 1024;
    unitIndex += 1;
  } while (scaled >= 1024 && unitIndex < units.length - 1);
  return `${scaled.toFixed(1)} ${units[unitIndex]}`;
}
