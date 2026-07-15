// 本文件单测节点流量详情纯逻辑：本地时间区间计算与趋势柱高归一化。
// 用本地构造的 Date（new Date(y,m,d,...)）算期望值，保证断言与运行时区无关。
// 覆盖：今日/近7天/近30天的 from_ms/to_ms/bucket、自定义区间与反选、柱高归一化与空数组。
import { describe, expect, it } from 'vitest';
import type { NodeTrafficTrendPoint } from '@/services/api';
import {
  buildTrafficTrendBars,
  formatBucketLabel,
  resolveTrafficRange,
  startOfLocalDay,
} from '@/views/monitor/nodeTraffic';

// 固定「现在」：本地 2026-06-27 15:30:45（用本地构造器，跨时区断言一致）。
const NOW = new Date(2026, 5, 27, 15, 30, 45, 0).getTime();
const localMidnight = (y: number, m: number, d: number) => new Date(y, m, d, 0, 0, 0, 0).getTime();

describe('startOfLocalDay', () => {
  it('取本地自然日 0 点', () => {
    expect(startOfLocalDay(NOW)).toBe(localMidnight(2026, 5, 27));
  });
});

describe('resolveTrafficRange - 今日', () => {
  it('from 为当天本地 0 点、to 为 now、桶为小时', () => {
    const range = resolveTrafficRange('today', NOW);
    expect(range.fromMs).toBe(localMidnight(2026, 5, 27));
    expect(range.toMs).toBe(NOW);
    expect(range.bucket).toBe('hour');
  });
});

describe('resolveTrafficRange - 近 7 天', () => {
  it('from 为 6 天前本地 0 点（含今天共 7 天）、桶为天', () => {
    const range = resolveTrafficRange('7d', NOW);
    expect(range.fromMs).toBe(localMidnight(2026, 5, 21));
    expect(range.toMs).toBe(NOW);
    expect(range.bucket).toBe('day');
  });
});

describe('resolveTrafficRange - 近 30 天', () => {
  it('from 为 29 天前本地 0 点（跨月正确）、桶为天', () => {
    const range = resolveTrafficRange('30d', NOW);
    // 2026-06-27 往前 29 天 = 2026-05-29。
    expect(range.fromMs).toBe(localMidnight(2026, 4, 29));
    expect(range.toMs).toBe(NOW);
    expect(range.bucket).toBe('day');
  });
});

describe('resolveTrafficRange - 自定义', () => {
  it('from 为起始日 0 点、to 为结束日次日 0 点（含尾日整天）', () => {
    const from = new Date(2026, 5, 10, 8, 0, 0).getTime();
    const to = new Date(2026, 5, 12, 20, 0, 0).getTime();
    const range = resolveTrafficRange('custom', NOW, from, to);
    expect(range.fromMs).toBe(localMidnight(2026, 5, 10));
    expect(range.toMs).toBe(localMidnight(2026, 5, 13));
    // 跨度 3 天 > 2 天阈值 → 天桶。
    expect(range.bucket).toBe('day');
  });

  it('单日自定义（跨度 ≤2 天）用小时桶', () => {
    const from = new Date(2026, 5, 10, 8, 0, 0).getTime();
    const to = new Date(2026, 5, 10, 20, 0, 0).getTime();
    const range = resolveTrafficRange('custom', NOW, from, to);
    expect(range.bucket).toBe('hour');
  });

  it('反选（起 > 止）自动纠正为非负区间', () => {
    const later = new Date(2026, 5, 12, 0, 0, 0).getTime();
    const earlier = new Date(2026, 5, 10, 0, 0, 0).getTime();
    const range = resolveTrafficRange('custom', NOW, later, earlier);
    expect(range.fromMs).toBe(localMidnight(2026, 5, 10));
    expect(range.toMs).toBe(localMidnight(2026, 5, 13));
  });

  it('缺端点回退今日区间', () => {
    const range = resolveTrafficRange('custom', NOW, null, null);
    expect(range.fromMs).toBe(localMidnight(2026, 5, 27));
    expect(range.toMs).toBe(NOW);
    expect(range.bucket).toBe('hour');
  });
});

describe('buildTrafficTrendBars - 柱高归一化', () => {
  const point = (over: Partial<NodeTrafficTrendPoint>): NodeTrafficTrendPoint => ({
    bucketStartMs: 0,
    uplinkBytes: 0,
    downlinkBytes: 0,
    totalBytes: 0,
    ...over,
  });

  it('最大总量的桶为 100%、0 值为 0%', () => {
    const bars = buildTrafficTrendBars([
      point({ bucketStartMs: 1, uplinkBytes: 40, downlinkBytes: 60, totalBytes: 100 }),
      point({ bucketStartMs: 2, uplinkBytes: 10, downlinkBytes: 10, totalBytes: 20 }),
      point({ bucketStartMs: 3, totalBytes: 0 }),
    ]);
    expect(bars[0].heightPercent).toBe(100);
    expect(bars[1].heightPercent).toBe(20);
    expect(bars[2].heightPercent).toBe(0);
  });

  it('柱内上/下行段占比按上行+下行拆分且和为 100', () => {
    const bars = buildTrafficTrendBars([
      point({ uplinkBytes: 30, downlinkBytes: 90, totalBytes: 120 }),
    ]);
    expect(bars[0].uplinkPercent).toBe(25);
    expect(bars[0].downlinkPercent).toBe(75);
  });

  it('全 0 时不除零：高度与占比都为 0', () => {
    const bars = buildTrafficTrendBars([point({ totalBytes: 0 }), point({ totalBytes: 0 })]);
    expect(bars.every((bar) => bar.heightPercent === 0 && bar.uplinkPercent === 0 && bar.downlinkPercent === 0)).toBe(true);
  });

  it('空数组安全返回空', () => {
    expect(buildTrafficTrendBars([])).toEqual([]);
  });
});

describe('formatBucketLabel', () => {
  it('小时桶显示 HH:00', () => {
    expect(formatBucketLabel(new Date(2026, 5, 27, 9, 0, 0).getTime(), 'hour')).toBe('09:00');
  });

  it('天桶显示 MM-DD', () => {
    expect(formatBucketLabel(new Date(2026, 5, 3, 0, 0, 0).getTime(), 'day')).toBe('06-03');
  });
});
