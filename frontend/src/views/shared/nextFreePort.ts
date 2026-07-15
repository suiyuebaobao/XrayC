// 本文件提供入口 / 本机出口「智能默认端口」的纯函数：给定连接方式与已占用端口集，
// 返回一个「合法且未占用」的端口，帮用户避开「端口已被占用」的后端 400。
// - CF 橙云：只在 CF 支持的 HTTPS 端口里挑（填别的到 CF 边缘连不上）。
// - IP 直连 / 灰云直连（对外入口）：优先防封友好端口，全占用再从 10000 起顺序找空的。
// - 本机出口（节点内部回环出口，不需防封）：直接从 10000 起用高端口，绝不抢稀缺的防封端口
//   （否则本机出口先占 443，用户再建 CF 入口又撞 443——正是要解决的痛点重演）。
// 仅做纯计算，不请求接口、不持有页面状态；后端端口唯一性护栏仍为最终兜底。

// 端口挑选的连接方式：cf=CF 橙云（端口受限）；ip/domain=对外入口（防封优先→10000+）；
// local-exit=本机出口（直接 10000+ 高端口，把稀缺的防封端口留给对外入口）。
export type PortSelectionMode = 'cf' | 'ip' | 'domain' | 'local-exit';

// CF 橙云候选端口：仅 Cloudflare 代理放行的 HTTPS 端口（官方口径），与 CF_SUPPORTED_HTTPS_PORTS 同集合。
export const CF_PORT_CANDIDATES = [443, 8443, 2053, 2083, 2087, 2096] as const;

// 对外入口（IP 直连 / 灰云直连）候选端口：优先防封友好的常见 HTTPS/HTTP 端口，降低被 DPI 盯上的概率。
// 本机出口不用这个列表（走高端口），把这些稀缺端口留给对外入口。
export const DIRECT_PORT_CANDIDATES = [
  443, 8443, 2053, 2083, 2087, 2096, 8080, 2052, 2082, 2086, 2095,
] as const;

// 高端口起点：本机出口默认从这里起找；对外入口在防封端口全占用时也回退到这里，避开低段常用/特权端口。
const FALLBACK_START_PORT = 10000;
const MAX_PORT = 65535;

// 把任意可迭代端口集合归一成只含正整数的 Set，便于用 has 做占用判定。
function toUsedSet(usedPorts: Iterable<number>): Set<number> {
  const used = new Set<number>();
  for (const port of usedPorts) {
    if (Number.isInteger(port) && port > 0) {
      used.add(port);
    }
  }
  return used;
}

// 给定连接方式 + 已占用端口集，返回第一个「合法且未占用」的端口；无可用端口返回 null。
// - CF 橙云：只在 CF_PORT_CANDIDATES 里挑第一个空的；6 个全占用返回 null（无可用 CF 端口）。
// - IP 直连 / 灰云直连（对外入口）：先在 DIRECT_PORT_CANDIDATES 挑第一个空的；全占用再从 10000 起找空端口。
// - 本机出口：跳过防封列表，直接从 10000 起找第一个空端口（防封端口留给对外入口）。
export function nextFreePort(mode: PortSelectionMode, usedPorts: Iterable<number>): number | null {
  const used = toUsedSet(usedPorts);
  if (mode === 'cf') {
    return CF_PORT_CANDIDATES.find((port) => !used.has(port)) ?? null;
  }
  // 本机出口不抢防封端口：只有对外入口（ip/domain）才先在防封列表里挑。
  if (mode !== 'local-exit') {
    const preferred = DIRECT_PORT_CANDIDATES.find((port) => !used.has(port));
    if (preferred !== undefined) {
      return preferred;
    }
  }
  for (let port = FALLBACK_START_PORT; port <= MAX_PORT; port += 1) {
    if (!used.has(port)) {
      return port;
    }
  }
  return null;
}

// 即时校验文案：用户手填的端口若非法（CF 选了非 CF 端口）或已被占用，返回红字提示；合法返回空串。
// 与 nextFreePort 同口径，供入口 / 本机出口表单在提交前就地提示，别等后端 400。
export function portConflictMessage(
  mode: PortSelectionMode,
  port: number,
  usedPorts: Set<number>,
): string {
  if (!Number.isInteger(port) || port < 1 || port > MAX_PORT) {
    return '';
  }
  // CF 橙云端口必须落在 CF 支持的 HTTPS 端口集合内，否则客户端到 CF 边缘超时连不上。
  if (mode === 'cf' && !(CF_PORT_CANDIDATES as readonly number[]).includes(port)) {
    return `CF 仅支持 ${CF_PORT_CANDIDATES.join('/')}`;
  }
  if (usedPorts.has(port)) {
    return '该端口已被占用，请换一个';
  }
  return '';
}
