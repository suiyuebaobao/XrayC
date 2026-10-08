// 防止把配置同步误报成实际可连接，并覆盖禁用、离线和缺失依赖。
import { describe, expect, it } from "vitest";
import { endpointAddress, lineConfigStatus } from "./lineStatus";
const entry = { enabled: true };
const exit = { enabled: true, exitResourceEnabled: true };
const node = { status: "healthy", healthStatus: "healthy" as const, configDirty: false, configSynced: true };
describe("line state", () => {
  it("labels synchronized configuration without claiming client connectivity", () => {
    expect(lineConfigStatus(true, entry, node, exit).label).toBe("已同步");
  });
  it("handles missing, disabled, offline and pending states", () => {
    expect(lineConfigStatus(true, entry, node).label).toBe("关联缺失");
    expect(lineConfigStatus(true, entry, node, { ...exit, enabled: false }).label).toBe("已停用");
    expect(lineConfigStatus(true, entry, { ...node, status: "offline" }, exit).label).toBe("节点离线");
    expect(lineConfigStatus(true, entry, { ...node, configDirty: true }, exit).label).toBe("待同步");
  });
  it("formats IPv6 and missing published addresses explicitly", () => {
    expect(endpointAddress("2001:db8::1", 443)).toBe("[2001:db8::1]:443");
    expect(endpointAddress("", 443)).toBe("地址未配置");
  });
});
