// 中文说明：数据库备份页纯逻辑单测，覆盖调度 kind 切换保值、间隔/cron 校验、
// 脱敏占位文案、收件人解析去重、状态映射等边界（node 环境，无 DOM）。
import { describe, expect, it } from 'vitest';
import type { BackupSchedule } from '@/services/api';
import {
  SECRET_SET_PLACEHOLDER,
  SECRET_UNSET_PLACEHOLDER,
  applyScheduleKind,
  backupStatusLabel,
  backupStatusTagType,
  describeSchedule,
  formatBackupTime,
  formatBytes,
  formatRecipients,
  isAdvancedCron,
  isCronExpressionValid,
  isScheduleValid,
  parseRecipients,
  runModeLabel,
  sanitizeIntervalMinutes,
  scheduleFromForm,
  scheduleToForm,
  secretPlaceholder,
  type ScheduleForm,
} from '@/views/backup/backupSchedule';

const interval = (over: Partial<BackupSchedule> = {}): BackupSchedule => ({
  kind: 'interval',
  intervalMinutes: 1440,
  cron: '',
  ...over,
});

describe('secretPlaceholder - 脱敏占位', () => {
  it('已设置显示留空不改，未设置提示请输入', () => {
    expect(secretPlaceholder(true)).toBe(SECRET_SET_PLACEHOLDER);
    expect(secretPlaceholder(false)).toBe(SECRET_UNSET_PLACEHOLDER);
  });
});

describe('applyScheduleKind - 调度 kind 切换保值', () => {
  it('从 interval 切到 cron 保留已填的 cron 与间隔值', () => {
    const next = applyScheduleKind(interval({ intervalMinutes: 30, cron: '0 3 * * *' }), 'cron');
    expect(next.kind).toBe('cron');
    expect(next.cron).toBe('0 3 * * *');
    expect(next.intervalMinutes).toBe(30);
  });

  it('从 cron 切回 interval 保留 cron 且归一非法间隔到下限 1', () => {
    const next = applyScheduleKind({ kind: 'cron', intervalMinutes: 0, cron: '5 4 * * *' }, 'interval');
    expect(next.kind).toBe('interval');
    expect(next.intervalMinutes).toBe(1);
    expect(next.cron).toBe('5 4 * * *');
  });
});

describe('sanitizeIntervalMinutes - 间隔归一', () => {
  it('取整并保证 ≥1', () => {
    expect(sanitizeIntervalMinutes(90.7)).toBe(90);
    expect(sanitizeIntervalMinutes(0)).toBe(1);
    expect(sanitizeIntervalMinutes(-5)).toBe(1);
    expect(sanitizeIntervalMinutes(Number.NaN)).toBe(1);
  });
});

describe('isCronExpressionValid - cron 结构校验', () => {
  it('恰好 5 段有效，多段/少段/空无效', () => {
    expect(isCronExpressionValid('0 3 * * *')).toBe(true);
    expect(isCronExpressionValid('  0   3 * * * ')).toBe(true);
    expect(isCronExpressionValid('0 3 * *')).toBe(false);
    expect(isCronExpressionValid('0 3 * * * *')).toBe(false);
    expect(isCronExpressionValid('')).toBe(false);
  });
});

describe('isScheduleValid - 整体调度校验', () => {
  it('interval 需 ≥1，cron 需 5 段', () => {
    expect(isScheduleValid(interval({ intervalMinutes: 10 }))).toBe(true);
    expect(isScheduleValid(interval({ intervalMinutes: 0 }))).toBe(false);
    expect(isScheduleValid({ kind: 'cron', intervalMinutes: 1, cron: '0 3 * * *' })).toBe(true);
    expect(isScheduleValid({ kind: 'cron', intervalMinutes: 1, cron: 'bad' })).toBe(false);
  });
});

describe('describeSchedule - 调度摘要', () => {
  it('间隔与 cron 分别输出人类可读文案', () => {
    expect(describeSchedule(interval({ intervalMinutes: 1440 }))).toBe('每 1440 分钟');
    expect(describeSchedule({ kind: 'cron', intervalMinutes: 1, cron: '0 3 * * *' })).toBe('Cron：0 3 * * *');
    expect(describeSchedule({ kind: 'cron', intervalMinutes: 1, cron: '' })).toBe('Cron：未设置');
  });
});

// 表单构造器：默认值齐全，按需覆盖，供四档生成/解析测试复用。
const form = (over: Partial<ScheduleForm> = {}): ScheduleForm => ({
  mode: 'daily',
  intervalNum: 1,
  intervalUnit: 'hour',
  time: '03:00',
  weekday: 1,
  monthday: 1,
  ...over,
});

describe('scheduleFromForm - 四档中文选项生成后端 schedule', () => {
  it('按间隔：分钟直取、小时×60', () => {
    expect(scheduleFromForm('interval', form({ intervalNum: 30, intervalUnit: 'minute' }))).toEqual({
      kind: 'interval',
      intervalMinutes: 30,
      cron: '',
    });
    expect(scheduleFromForm('interval', form({ intervalNum: 6, intervalUnit: 'hour' })).intervalMinutes).toBe(360);
  });

  it('每天：生成 "m h * * *"', () => {
    const s = scheduleFromForm('daily', form({ time: '03:30' }));
    expect(s.kind).toBe('cron');
    expect(s.cron).toBe('30 3 * * *');
  });

  it('每周：生成 "m h * * w"（周日=0…周六=6）', () => {
    expect(scheduleFromForm('weekly', form({ time: '08:05', weekday: 0 })).cron).toBe('5 8 * * 0');
    expect(scheduleFromForm('weekly', form({ time: '23:00', weekday: 6 })).cron).toBe('0 23 * * 6');
  });

  it('每月：生成 "m h d * *"（d=1..28）', () => {
    expect(scheduleFromForm('monthly', form({ time: '01:00', monthday: 15 })).cron).toBe('0 1 15 * *');
  });

  it('非法星期/号/间隔归一：weekday>6→1、monthday>28→1、间隔<1→1', () => {
    expect(scheduleFromForm('weekly', form({ time: '00:00', weekday: 9 })).cron).toBe('0 0 * * 1');
    expect(scheduleFromForm('monthly', form({ time: '00:00', monthday: 31 })).cron).toBe('0 0 1 * *');
    expect(scheduleFromForm('interval', form({ intervalNum: 0, intervalUnit: 'minute' })).intervalMinutes).toBe(1);
  });
});

describe('scheduleToForm - 后端 schedule 解析回显', () => {
  it('interval：能整除 60 显示小时，否则分钟', () => {
    expect(scheduleToForm({ kind: 'interval', intervalMinutes: 120, cron: '' })).toMatchObject({
      mode: 'interval',
      intervalNum: 2,
      intervalUnit: 'hour',
    });
    expect(scheduleToForm({ kind: 'interval', intervalMinutes: 45, cron: '' })).toMatchObject({
      mode: 'interval',
      intervalNum: 45,
      intervalUnit: 'minute',
    });
  });

  it('每天 cron 解析出时间', () => {
    expect(scheduleToForm({ kind: 'cron', intervalMinutes: 1, cron: '30 3 * * *' })).toMatchObject({
      mode: 'daily',
      time: '03:30',
    });
  });

  it('每周 cron 解析出星期与时间', () => {
    expect(scheduleToForm({ kind: 'cron', intervalMinutes: 1, cron: '5 8 * * 0' })).toMatchObject({
      mode: 'weekly',
      weekday: 0,
      time: '08:05',
    });
  });

  it('每月 cron 解析出号与时间', () => {
    expect(scheduleToForm({ kind: 'cron', intervalMinutes: 1, cron: '0 1 15 * *' })).toMatchObject({
      mode: 'monthly',
      monthday: 15,
      time: '01:00',
    });
  });

  it('复杂/历史 cron 兜底回退按间隔（每天=1440 分钟=24 小时），不崩', () => {
    // 步进、多值、范围、周和日同时限定、月份非通配都算「高级」，一律回退。
    for (const cron of ['*/15 * * * *', '0 3 1,15 * *', '0 3 * * 1-5', '0 3 1 * 1', '0 3 1 6 *', 'garbage']) {
      expect(scheduleToForm({ kind: 'cron', intervalMinutes: 1, cron })).toMatchObject({
        mode: 'interval',
        intervalNum: 24,
        intervalUnit: 'hour',
      });
    }
  });
});

describe('scheduleFromForm ↔ scheduleToForm - 四档往返一致', () => {
  it('按间隔往返', () => {
    const back = scheduleToForm(scheduleFromForm('interval', form({ intervalNum: 90, intervalUnit: 'minute' })));
    expect(back).toMatchObject({ mode: 'interval', intervalNum: 90, intervalUnit: 'minute' });
  });

  it('每天/每周/每月往返 mode 与时间字段不变', () => {
    const daily = scheduleToForm(scheduleFromForm('daily', form({ time: '12:34' })));
    expect(daily).toMatchObject({ mode: 'daily', time: '12:34' });

    const weekly = scheduleToForm(scheduleFromForm('weekly', form({ time: '06:07', weekday: 3 })));
    expect(weekly).toMatchObject({ mode: 'weekly', weekday: 3, time: '06:07' });

    const monthly = scheduleToForm(scheduleFromForm('monthly', form({ time: '22:09', monthday: 28 })));
    expect(monthly).toMatchObject({ mode: 'monthly', monthday: 28, time: '22:09' });
  });
});

describe('isAdvancedCron - 高级/历史 cron 判定', () => {
  it('简单三档不算高级，复杂表达式与非 cron 类型判定正确', () => {
    expect(isAdvancedCron({ kind: 'cron', intervalMinutes: 1, cron: '0 3 * * *' })).toBe(false);
    expect(isAdvancedCron({ kind: 'cron', intervalMinutes: 1, cron: '0 3 * * 5' })).toBe(false);
    expect(isAdvancedCron({ kind: 'cron', intervalMinutes: 1, cron: '0 3 10 * *' })).toBe(false);
    expect(isAdvancedCron({ kind: 'cron', intervalMinutes: 1, cron: '*/15 * * * *' })).toBe(true);
    expect(isAdvancedCron({ kind: 'cron', intervalMinutes: 1, cron: '0 3 1-5 * *' })).toBe(true);
    // interval 类型永远不算高级 cron。
    expect(isAdvancedCron({ kind: 'interval', intervalMinutes: 60, cron: '' })).toBe(false);
  });
});

describe('parseRecipients / formatRecipients - 收件人', () => {
  it('多分隔符切分、trim、去重、去空并保持顺序', () => {
    expect(parseRecipients('a@x.com, b@x.com\n a@x.com ;c@x.com')).toEqual([
      'a@x.com',
      'b@x.com',
      'c@x.com',
    ]);
    expect(parseRecipients('   ')).toEqual([]);
  });

  it('数组回填为一行一个', () => {
    expect(formatRecipients(['a@x.com', 'b@x.com'])).toBe('a@x.com\nb@x.com');
  });
});

describe('runModeLabel / 状态映射', () => {
  it('模式中文名', () => {
    expect(runModeLabel('full')).toBe('全量备份');
    expect(runModeLabel('offsite')).toBe('异地同步');
    expect(runModeLabel('wal')).toBe('WAL 基准备份');
    expect(runModeLabel('email')).toBe('邮件发送');
  });

  it('状态文案与标签色', () => {
    expect(backupStatusLabel('success')).toBe('成功');
    expect(backupStatusLabel('failed')).toBe('失败');
    expect(backupStatusLabel('')).toBe('未运行');
    expect(backupStatusTagType('success')).toBe('success');
    expect(backupStatusTagType('failed')).toBe('danger');
    expect(backupStatusTagType('running')).toBe('warning');
    expect(backupStatusTagType('idle')).toBe('info');
  });
});

describe('formatBackupTime - 时间展示', () => {
  it('空串占位，非法原样返回', () => {
    expect(formatBackupTime('')).toBe('—');
    expect(formatBackupTime('not-a-date')).toBe('not-a-date');
  });

  it('合法 ISO 时间返回非空本地化串', () => {
    expect(formatBackupTime('2026-07-01T03:00:00Z').length).toBeGreaterThan(0);
  });
});

describe('formatBytes - 字节可读化', () => {
  it('小于 1KB 取整显示 B', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1023.7)).toBe('1024 B');
  });

  it('KB/MB/GB 保留 1 位小数', () => {
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB');
    expect(formatBytes(1024 ** 3)).toBe('1.0 GB');
    expect(formatBytes(1.5 * 1024 ** 3)).toBe('1.5 GB');
  });

  it('封顶 GB：TB 量级仍按 GB 展示', () => {
    expect(formatBytes(1024 ** 4)).toBe('1024.0 GB');
  });

  it('非有限值或负数按 0 处理', () => {
    expect(formatBytes(Number.NaN)).toBe('0 B');
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe('0 B');
    expect(formatBytes(-100)).toBe('0 B');
  });
});
