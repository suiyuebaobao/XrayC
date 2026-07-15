// 本文件是监控中心「节点流量详情」的纯逻辑：时间区间计算与趋势柱高归一化。
// 抽成纯函数便于 vitest 单测，不依赖 Vue / Element Plus / 网络请求。
// 区间计算按运行环境本地时区（当天 0 点、近 N 天从 N-1 天前的本地 0 点起算）。
// 柱高归一化：整根柱按区间最大总量归一化成 0..100 百分比，柱内再按上/下行拆两色段。
import type { NodeTrafficTrendPoint } from '@/services/api';

// 时间段档位：今日按小时桶，近 7 / 30 天按天桶，自定义按跨度自动选桶。
export type TrafficRangeKey = 'today' | '7d' | '30d' | 'custom';

// 趋势桶粒度：小时 / 天，对齐后端 bucket 查询参数。
export type TrafficBucket = 'hour' | 'day';

// 解析出的时间区间：起止毫秒（左闭右开）+ 采用的桶粒度。
export type ResolvedTrafficRange = {
  fromMs: number;
  toMs: number;
  bucket: TrafficBucket;
};

// 一天的毫秒数，用于自定义区间跨度判定（不用它做「N 天前」偏移，偏移走本地日历避开 DST）。
const DAY_MS = 24 * 60 * 60 * 1000;
// 自定义区间跨度 ≤ 此阈值（默认 2 天）用小时桶，否则用天桶。
const HOUR_BUCKET_MAX_SPAN_MS = 2 * DAY_MS;

// 取某毫秒时间戳所在「本地自然日」的 0 点毫秒（按运行环境本地时区）。
export function startOfLocalDay(ms: number): number {
  const date = new Date(ms);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

// 以某毫秒所在本地自然日的 0 点为基准，按本地日历偏移 deltaDays 天再取 0 点。
// 走 setDate 而非固定毫秒相减，避免跨夏令时（DST）时「N 天前」落不到本地 0 点。
function shiftLocalDays(ms: number, deltaDays: number): number {
  const date = new Date(ms);
  date.setHours(0, 0, 0, 0);
  date.setDate(date.getDate() + deltaDays);
  return date.getTime();
}

// 按档位算起止毫秒与桶粒度：
// - today：当天本地 0 点 → now，小时桶；
// - 7d/30d：从 (N-1) 天前的本地 0 点 → now，天桶（含今天共 N 天）；
// - custom：起始日本地 0 点 → 结束日次日 0 点（含尾日整天），跨度 ≤2 天用小时桶否则天桶；
//   缺任一端点时回退今日，避免把 NaN 区间打到后端。
export function resolveTrafficRange(
  key: TrafficRangeKey,
  now: number,
  customFrom?: number | null,
  customTo?: number | null,
): ResolvedTrafficRange {
  if (key === 'today') {
    return { fromMs: startOfLocalDay(now), toMs: now, bucket: 'hour' };
  }
  if (key === '7d' || key === '30d') {
    const days = key === '7d' ? 7 : 30;
    return { fromMs: shiftLocalDays(now, -(days - 1)), toMs: now, bucket: 'day' };
  }
  if (customFrom === null || customFrom === undefined || customTo === null || customTo === undefined) {
    return { fromMs: startOfLocalDay(now), toMs: now, bucket: 'hour' };
  }
  // 用户可能反选起止，统一按小/大排序，保证左闭右开区间非负。
  const startMs = Math.min(customFrom, customTo);
  const endMs = Math.max(customFrom, customTo);
  const fromMs = startOfLocalDay(startMs);
  // 结束日含整天：取结束日次日 0 点作为开区间右端。
  const toMs = shiftLocalDays(endMs, 1);
  const bucket: TrafficBucket = toMs - fromMs <= HOUR_BUCKET_MAX_SPAN_MS ? 'hour' : 'day';
  return { fromMs, toMs, bucket };
}

// 单根趋势柱：原始上/下/总字节 + 归一化后的整柱高度百分比 + 柱内上/下行段占比。
export type TrafficTrendBar = {
  bucketStartMs: number;
  uplinkBytes: number;
  downlinkBytes: number;
  totalBytes: number;
  // 整根柱相对区间最大总量的高度（0..100）：最大总量的桶为 100，全 0 或空区间为 0。
  heightPercent: number;
  // 柱内上行段高度占本柱的百分比（0..100），用于两色叠加着色；本柱总量为 0 时为 0。
  uplinkPercent: number;
  // 柱内下行段高度占本柱的百分比（0..100）。
  downlinkPercent: number;
};

// 把后端趋势点归一化成可直接渲染的柱：整柱高按区间最大总量归一化，柱内再按上/下行拆两色段。
// 空数组安全返回空；最大总量为 0（全 0）时所有柱高度 0，不抛异常。
export function buildTrafficTrendBars(points: NodeTrafficTrendPoint[]): TrafficTrendBar[] {
  const maxTotal = points.reduce((max, point) => Math.max(max, point.totalBytes), 0);
  return points.map((point) => {
    const heightPercent = maxTotal > 0 ? (point.totalBytes / maxTotal) * 100 : 0;
    // 柱内上/下行占比以「上行+下行」为基准（后端 total 可能另含开销，避免两段占比溢出）。
    const split = point.uplinkBytes + point.downlinkBytes;
    const uplinkPercent = split > 0 ? (point.uplinkBytes / split) * 100 : 0;
    return {
      bucketStartMs: point.bucketStartMs,
      uplinkBytes: point.uplinkBytes,
      downlinkBytes: point.downlinkBytes,
      totalBytes: point.totalBytes,
      heightPercent,
      uplinkPercent,
      downlinkPercent: split > 0 ? 100 - uplinkPercent : 0,
    };
  });
}

// 把桶起始毫秒格式化成柱下方短标签：小时桶显示 HH:00，天桶显示 MM-DD（本地时区）。
export function formatBucketLabel(bucketStartMs: number, bucket: TrafficBucket): string {
  const date = new Date(bucketStartMs);
  const pad = (value: number) => String(value).padStart(2, '0');
  if (bucket === 'hour') {
    return `${pad(date.getHours())}:00`;
  }
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}
