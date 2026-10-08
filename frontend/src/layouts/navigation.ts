// 工作区导航统一维护页面归属，旧路由保留可访问，避免选单与页签各自维护而错位。
export type Workspace = {
  id: string;
  label: string;
  icon: string;
  section: "日常管理" | "运维" | "系统";
  path: string;
  tabs: Array<{ label: string; path: string }>;
};

export const adminWorkspaces: Workspace[] = [
  { id: "overview", label: "总览", icon: "DataBoard", section: "日常管理", path: "/overview", tabs: [] },
  {
    id: "lines",
    label: "线路",
    icon: "Connection",
    section: "日常管理",
    path: "/admin/lines",
    tabs: [
      { label: "订阅线路", path: "/admin/lines" },
      { label: "入口", path: "/admin/access-entries" },
      { label: "出口", path: "/admin/line-pool" },
      { label: "分组", path: "/admin/line-groups" },
    ],
  },
  {
    id: "users",
    label: "用户",
    icon: "User",
    section: "日常管理",
    path: "/admin/users",
    tabs: [
      { label: "用户列表", path: "/admin/users" },
      { label: "流量明细", path: "/admin/traffic-logs" },
      { label: "邀请注册", path: "/admin/invite-codes" },
    ],
  },
  {
    id: "plans",
    label: "套餐与运营",
    icon: "Tickets",
    section: "日常管理",
    path: "/admin/plans",
    tabs: [
      { label: "套餐授权", path: "/admin/plans" },
      { label: "订单", path: "/admin/orders" },
      { label: "兑换码", path: "/admin/redeem-codes" },
      { label: "支付渠道", path: "/admin/payment-settings" },
    ],
  },
  { id: "nodes", label: "节点", icon: "SetUp", section: "运维", path: "/admin/transit-nodes", tabs: [] },
  {
    id: "monitor",
    label: "监控与日志",
    icon: "Monitor",
    section: "运维",
    path: "/admin/access-operations",
    tabs: [
      { label: "运行监控", path: "/admin/access-operations" },
      { label: "操作审计", path: "/admin/audit-logs" },
    ],
  },
  {
    id: "settings",
    label: "系统设置",
    icon: "Setting",
    section: "系统",
    path: "/admin/subscription-settings",
    tabs: [
      { label: "订阅", path: "/admin/subscription-settings" },
      { label: "分流规则", path: "/admin/rule-settings" },
      { label: "认证与邮件", path: "/admin/auth-security" },
      { label: "备份", path: "/admin/database-backup" },
      { label: "网站", path: "/admin/sales-landing" },
      { label: "版本与运行", path: "/admin/system-info" },
    ],
  },
];

export const userNavigation = [
  { label: "我的服务", path: "/dashboard", icon: "DataBoard" },
  { label: "套餐", path: "/plans", icon: "Tickets" },
  { label: "我的订单", path: "/orders", icon: "Document" },
  { label: "兑换码", path: "/redeem", icon: "Present" },
  { label: "邀请码", path: "/invite-codes", icon: "UserFilled" },
  { label: "账户安全", path: "/account/password", icon: "Lock" },
];

export function workspaceForPath(path: string): Workspace | undefined {
  if (path === "/admin/access-routing") return adminWorkspaces.find((item) => item.id === "nodes");
  if (path === "/admin/health-check") return adminWorkspaces.find((item) => item.id === "monitor");
  return adminWorkspaces.find((item) => item.path === path || item.tabs.some((tab) => tab.path === path));
}
