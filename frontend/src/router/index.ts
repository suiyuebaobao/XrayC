// 本文件定义前端路由表和登录守卫。
// 它维护访客页、用户后台和管理员后台的路径入口。
// 路由守卫只读取 session store，不直接发起登录请求。
// 新增页面时必须在这里明确 meta 权限要求。
import { createRouter, createWebHistory } from 'vue-router';
import { useSessionStore } from '@/stores/session';

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'sales-landing',
      component: () => import('@/views/SalesLandingPage.vue'),
      meta: { public: true },
    },
    {
      path: '/platform',
      name: 'platform',
      component: () => import('@/views/PlatformPage.vue'),
      meta: { public: true },
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginPage.vue'),
      meta: { public: true },
    },
    {
      path: '/admin/login',
      name: 'admin-login',
      component: () => import('@/views/LoginPage.vue'),
      meta: { public: true },
    },
    {
      path: '/register',
      name: 'register',
      component: () => import('@/views/RegisterPage.vue'),
      meta: { public: true },
    },
    {
      path: '/',
      component: () => import('@/layouts/AppShell.vue'),
      children: [
        {
          path: 'admin',
          redirect: '/overview',
        },
        {
          path: 'dashboard',
          name: 'dashboard',
          component: () => import('@/views/DashboardPage.vue'),
          meta: { user: true },
        },
        {
          path: 'overview',
          name: 'overview',
          component: () => import('@/views/OverviewPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/tutorial',
          name: 'admin-tutorial',
          component: () => import('@/views/AdminTutorialPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/access-operations',
          name: 'access-operations',
          component: () => import('@/views/MonitorCenterPage.vue'),
          meta: { admin: true },
        },
        {
          // 健康检查已并入监控中心，旧路径重定向到监控中心保兼容（书签/外链仍可达）。
          path: 'admin/health-check',
          name: 'health-check',
          redirect: { name: 'access-operations' },
          meta: { admin: true },
        },
        {
          path: 'admin/transit-nodes',
          name: 'transit-nodes',
          component: () => import('@/views/AccessLinesPage.vue'),
          alias: ['/admin/access-routing'],
          meta: { admin: true },
        },
        {
          path: 'admin/access-entries',
          name: 'access-entries',
          component: () => import('@/views/AccessEntriesPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/line-pool',
          name: 'line-pool',
          component: () => import('@/views/LinePoolPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/plans',
          name: 'admin-plans',
          component: () => import('@/views/PlansPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/sales-landing',
          name: 'admin-sales-landing',
          component: () => import('@/views/AdminSalesLandingPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/users',
          name: 'admin-users',
          component: () => import('@/views/UsersPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/traffic-logs',
          name: 'admin-traffic-logs',
          component: () => import('@/views/AdminTrafficLogsPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/orders',
          name: 'admin-orders',
          component: () => import('@/views/OrdersPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/redeem-codes',
          name: 'admin-redeem-codes',
          component: () => import('@/views/RedeemCodesPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/invite-codes',
          name: 'admin-invite-codes',
          component: () => import('@/views/AdminInviteCodesPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/auth-security',
          name: 'admin-auth-security',
          component: () => import('@/views/AuthSecurityPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/payment-settings',
          name: 'admin-payment-settings',
          component: () => import('@/views/PaymentSettingsPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/audit-logs',
          name: 'admin-audit-logs',
          component: () => import('@/views/AuditLogsPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/database-backup',
          name: 'admin-database-backup',
          component: () => import('@/views/backup/DatabaseBackupPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/subscription-settings',
          name: 'admin-subscription-settings',
          component: () => import('@/views/SubscriptionSettingsPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/line-groups',
          name: 'line-groups',
          component: () => import('@/views/LineGroupsPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'admin/rule-settings',
          name: 'admin-rule-settings',
          component: () => import('@/views/RuleSettingsPage.vue'),
          meta: { admin: true },
        },
        {
          path: 'plans',
          name: 'plans',
          component: () => import('@/views/UserPlansPage.vue'),
          meta: { user: true },
        },
        {
          path: 'subscription',
          name: 'subscription',
          component: () => import('@/views/SubscriptionPage.vue'),
          meta: { user: true },
        },
        {
          path: 'orders',
          name: 'user-orders',
          component: () => import('@/views/UserOrdersPage.vue'),
          meta: { user: true },
        },
        {
          path: 'redeem',
          name: 'redeem',
          component: () => import('@/views/RedeemPage.vue'),
          meta: { user: true },
        },
        {
          path: 'invite-codes',
          name: 'invite-codes',
          component: () => import('@/views/InviteCodesPage.vue'),
          meta: { user: true },
        },
        {
          path: 'account/password',
          name: 'change-password',
          component: () => import('@/views/ChangePasswordPage.vue'),
          meta: { user: true },
        },
      ],
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: '/',
    },
  ],
});

router.beforeEach((to) => {
  const session = useSessionStore();
  if (!to.meta.public && !session.isAuthenticated) {
    return {
      name: to.fullPath.startsWith('/admin') ? 'admin-login' : 'login',
      query: { redirect: to.fullPath },
    };
  }

  if (to.meta.admin && !session.isAdmin) {
    return { name: 'dashboard' };
  }

  if (to.meta.user && session.isAdmin) {
    return { name: 'overview' };
  }

  if ((to.name === 'login' || to.name === 'admin-login' || to.name === 'register') && session.isAuthenticated) {
    return session.isAdmin ? { name: 'overview' } : { name: 'dashboard' };
  }

  return true;
});
