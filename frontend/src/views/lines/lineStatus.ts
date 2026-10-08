// 将配置状态与连通性分开：Agent 在线/配置同步不能被显示为客户端实测可用。
import type { AccessEntrySummary, AccessNodeSummary, ExitEndpointSummary } from "@/services/api";
type Entry = Pick<AccessEntrySummary, "enabled">;
type Node = Pick<AccessNodeSummary, "status" | "healthStatus" | "configDirty" | "configSynced">;
type Exit = Pick<ExitEndpointSummary, "enabled" | "exitResourceEnabled">;
export function lineConfigStatus(enabled: boolean, entry?: Entry, node?: Node, exit?: Exit) {
  if (!entry || !node || !exit) return { label: "关联缺失", type: "danger" as const };
  if (!enabled || !entry.enabled || !exit.enabled || !exit.exitResourceEnabled) return { label: "已停用", type: "info" as const };
  if (node.status === "offline" || node.healthStatus === "offline") return { label: "节点离线", type: "danger" as const };
  if (node.configDirty || !node.configSynced) return { label: "待同步", type: "warning" as const };
  return { label: "已同步", type: "success" as const };
}
export function endpointAddress(host: string, port: number) {
  if (!host) return "地址未配置";
  return `${host.includes(":") && !host.startsWith("[") ? `[${host}]` : host}:${port}`;
}
