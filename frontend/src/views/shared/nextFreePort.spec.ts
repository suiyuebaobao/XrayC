// 中文说明：智能默认端口纯函数单测，覆盖 CF 全占用返回 null、防封端口全占用回退 10000+、
// 编辑态保留（排除自身端口）等边界，以及即时校验文案与节点已用端口聚合口径。
import { describe, expect, it } from 'vitest';
import {
  CF_PORT_CANDIDATES,
  DIRECT_PORT_CANDIDATES,
  nextFreePort,
  portConflictMessage,
} from '@/views/shared/nextFreePort';
import { collectNodeUsedPorts } from '@/views/shared/nodeUsedPorts';
import type {
  AccessEntrySummary,
  ExitEndpointSummary,
  ExitResourceSummary,
} from '@/services/api';

describe('nextFreePort - CF 橙云', () => {
  it('无占用时取首个 CF 端口 443', () => {
    expect(nextFreePort('cf', [])).toBe(443);
  });

  it('443 被占用时取下一个 CF 端口 8443', () => {
    expect(nextFreePort('cf', [443])).toBe(8443);
  });

  it('前几个 CF 端口被占用时跳到首个空的', () => {
    expect(nextFreePort('cf', [443, 8443, 2053])).toBe(2083);
  });

  it('CF 端口全占用返回 null（无可用 CF 端口，不回退别的端口）', () => {
    expect(nextFreePort('cf', [...CF_PORT_CANDIDATES])).toBeNull();
  });
});

describe('nextFreePort - IP直连 / 灰云直连（对外入口）', () => {
  it('无占用时优先防封端口 443', () => {
    expect(nextFreePort('ip', [])).toBe(443);
    expect(nextFreePort('domain', [])).toBe(443);
  });

  it('443 被占用时取下一个防封端口 8443', () => {
    expect(nextFreePort('ip', [443])).toBe(8443);
  });

  it('防封端口全占用时从 10000 起找第一个空的', () => {
    expect(nextFreePort('ip', [...DIRECT_PORT_CANDIDATES])).toBe(10000);
  });

  it('防封端口全占用且 10000/10001 也占用时取 10002', () => {
    expect(nextFreePort('ip', [...DIRECT_PORT_CANDIDATES, 10000, 10001])).toBe(10002);
  });

  it('忽略非法端口值（0/负数/非整数）', () => {
    expect(nextFreePort('ip', [0, -1, 1.5])).toBe(443);
  });
});

describe('nextFreePort - 本机出口用高端口（不抢防封端口）', () => {
  it('无占用时直接用 10000（防封端口全空也不用，留给对外入口）', () => {
    expect(nextFreePort('local-exit', [])).toBe(10000);
  });

  it('返回值绝不落在防封端口列表里（即使这些口空着）', () => {
    const port = nextFreePort('local-exit', []) as number;
    expect(port).toBeGreaterThanOrEqual(10000);
    expect(DIRECT_PORT_CANDIDATES).not.toContain(port);
  });

  it('10000/10001 占用时取 10002（防封端口占不占都无所谓）', () => {
    expect(nextFreePort('local-exit', [10000, 10001])).toBe(10002);
    expect(nextFreePort('local-exit', [...DIRECT_PORT_CANDIDATES, 10000, 10001])).toBe(10002);
  });
});

describe('portConflictMessage - 即时校验文案', () => {
  it('合法未占用端口无提示', () => {
    expect(portConflictMessage('ip', 443, new Set())).toBe('');
  });

  it('已占用端口红字提示', () => {
    expect(portConflictMessage('ip', 443, new Set([443]))).toBe('该端口已被占用，请换一个');
  });

  it('CF 选了非 CF 端口时提示只支持 CF 端口', () => {
    expect(portConflictMessage('cf', 8080, new Set())).toBe('CF 仅支持 443/8443/2053/2083/2087/2096');
  });

  it('CF 选了合法 CF 端口但已被占用仍提示占用', () => {
    expect(portConflictMessage('cf', 443, new Set([443]))).toBe('该端口已被占用，请换一个');
  });
});

describe('collectNodeUsedPorts - 节点已用端口聚合', () => {
  const entry = (over: Partial<AccessEntrySummary>): AccessEntrySummary =>
    ({ id: 'e1', accessNodeId: 'n1', enabled: true, listenPort: 443, ...over }) as AccessEntrySummary;
  const resource = (over: Partial<ExitResourceSummary>): ExitResourceSummary =>
    ({ id: 'r1', accessNodeId: 'n1', ownership: 'self_hosted', ...over }) as ExitResourceSummary;
  const endpoint = (over: Partial<ExitEndpointSummary>): ExitEndpointSummary =>
    ({ id: 'p1', exitResourceId: 'r1', port: 1443, ...over }) as ExitEndpointSummary;

  it('并入启用入口 listen_port 与该节点自建出口 endpoint port', () => {
    const used = collectNodeUsedPorts(
      'n1',
      [entry({ id: 'e1', listenPort: 443 }), entry({ id: 'e2', listenPort: 8443 })],
      [endpoint({ id: 'p1', port: 1443 })],
      [resource({ id: 'r1' })],
    );
    expect([...used].sort((a, b) => a - b)).toEqual([443, 1443, 8443]);
  });

  it('停用入口与其它节点入口不计入', () => {
    const used = collectNodeUsedPorts(
      'n1',
      [
        entry({ id: 'e1', listenPort: 443, enabled: false }),
        entry({ id: 'e2', accessNodeId: 'n2', listenPort: 8443 }),
      ],
      [],
      [],
    );
    expect(used.size).toBe(0);
  });

  it('第三方出口（非 self_hosted / 别的节点）不占本节点端口', () => {
    const used = collectNodeUsedPorts(
      'n1',
      [],
      [endpoint({ id: 'p1', exitResourceId: 'r9', port: 9000 })],
      [resource({ id: 'r9', ownership: 'third_party', accessNodeId: '' })],
    );
    expect(used.size).toBe(0);
  });

  it('编辑态排除自身入口端口（不把自己的端口当占用）', () => {
    const used = collectNodeUsedPorts(
      'n1',
      [entry({ id: 'e1', listenPort: 443 })],
      [],
      [],
      'e1',
    );
    expect(used.has(443)).toBe(false);
  });
});
