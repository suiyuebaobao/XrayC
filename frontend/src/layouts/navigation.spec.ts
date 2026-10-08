// 导航整理必须保留旧管理路径与子页归属，不能把用户路由错归为管理员工作区。
import { describe, expect, it } from "vitest";
import { adminWorkspaces, workspaceForPath } from "./navigation";
describe("workspace navigation", () => {
  it("maps every workspace tab back to its parent", () => {
    for (const group of adminWorkspaces) {
      expect(workspaceForPath(group.path)?.id).toBe(group.id);
      for (const tab of group.tabs) expect(workspaceForPath(tab.path)?.id).toBe(group.id);
    }
  });
  it("keeps legacy aliases and separates user routes", () => {
    expect(workspaceForPath("/admin/access-routing")?.id).toBe("nodes");
    expect(workspaceForPath("/admin/health-check")?.id).toBe("monitor");
    expect(workspaceForPath("/subscription")).toBeUndefined();
  });
});
