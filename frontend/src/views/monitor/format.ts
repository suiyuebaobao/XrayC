// 本文件提供监控中心资源面板专用的展示格式化与占用率派生。
// 它只把后端千分比（pct_milli）换算成百分比展示，并按阈值给出告警颜色。
// 字节展示直接复用运营格式化里的 formatBytes，避免再实现一套单位换算。
// 这里不发请求、不持状态，只做纯函数转换，供平台资源/节点资源面板复用。
import type { NodeRuntimeMetrics } from '@/services/api';

const pctFormatter = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 1 });

export type UsageTone = 'success' | 'warning' | 'danger';

// 千分比（0..100000）→ 百分比数值（0..100），供 el-progress percentage 与环形占比使用。
export function pctMilliToPercent(pctMilli: number | null | undefined): number {
  if (pctMilli === null || pctMilli === undefined || !Number.isFinite(pctMilli)) {
    return 0;
  }
  const percent = pctMilli / 1000;
  return Math.min(100, Math.max(0, percent));
}

// 千分比 → 带百分号的展示文本（保留一位小数）。
export function formatPctMilli(pctMilli: number | null | undefined): string {
  if (pctMilli === null || pctMilli === undefined || !Number.isFinite(pctMilli)) {
    return '未上报';
  }
  return `${pctFormatter.format(pctMilliToPercent(pctMilli))}%`;
}

// 按使用率给进度/环形上色：<70% 健康、70%~90% 警告、≥90% 危险。
export function usageTone(percent: number): UsageTone {
  if (percent >= 90) {
    return 'danger';
  }
  if (percent >= 70) {
    return 'warning';
  }
  return 'success';
}

// Element Plus 进度条颜色映射，沿用主题色变量对应的成功/警告/危险色。
export function usageColor(percent: number): string {
  const tone = usageTone(percent);
  if (tone === 'danger') {
    return '#f56c6c';
  }
  return tone === 'warning' ? '#e6a23c' : '#67c23a';
}

// 由 used/total 字节算占用百分比（total 为 0 或缺失时按 0），用于节点资源卡片。
export function bytesUsagePercent(usedBytes: number, totalBytes: number): number {
  if (!totalBytes || totalBytes <= 0) {
    return 0;
  }
  return Math.min(100, Math.max(0, (usedBytes / totalBytes) * 100));
}

// 判断节点运行态指标是否有上报（null 即未上报/待 agent 升级）。
export function hasNodeRuntimeMetrics(value: NodeRuntimeMetrics | null): value is NodeRuntimeMetrics {
  return value !== null;
}
