import { expectAdminWorkspaces, openNodeOnboarding } from '../helpers/node-onboarding';
// 用途：覆盖管理员登录后的概览、中转节点、分组和后台页面 smoke 流程。
import { expect, test } from '@playwright/test';

import {
  expectDialogOpensAndCloses,
  mockAdminSession,
  requireRealAccessEndpoint,
} from '../helpers/smoke';

test('管理员可以登录并查看概览、中转节点和分组', async ({ page }) => {
  test.setTimeout(60_000);

  await mockAdminSession(page);
  await page.route('**/api/admin/access-operations/summary', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          active_users: 2,
          active_access_lines: 1,
          healthy_exit_pools: 1,
          monthly_billed_traffic_gb: 12,
          config_dirty_nodes: 0,
          access_node_count: 1,
          access_line_count: 1,
          exit_pool_count: 1,
          ledger_count: 0,
          generated_at: '2026-06-03T00:00:00Z',
          latest_metric_at: null,
          recent_events: [],
        },
      }),
    });
  });
  await page.route('**/api/admin/access-routing', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_nodes: [
            {
              id: 'node-smoke-1',
              name: '香港中转 01',
              public_host: 'access.example.test',
              config_dirty: false,
            },
          ],
          access_lines: [
            {
              id: 'line-smoke-1',
              uuid: 'line-smoke-1',
              name: '香港入口',
              access_node_id: 'node-smoke-1',
              access_node: '香港中转 01',
              exit_endpoint_id: 'exit-smoke-1',
              exit_endpoint_name: '出口管理出口 01',
              listen_host: 'access.example.test',
              listen_port: 443,
              inbound_protocol: 'vless+tcp',
              enabled: true,
              status: 'enabled',
            },
          ],
          exit_pools: [
            {
              id: 'pool-smoke-1',
              name: 'AI 分组出口集合',
              strategy: 'priority',
              healthy_members: 1,
              total_members: 1,
              members: [
                {
                  id: 'exit-smoke-1',
                  name: '出口管理出口 01',
                  endpoint_name: '出口管理出口 01',
                  outbound_type: 'socks',
                  host: 'upstream.example.test',
                  port: 1080,
                  healthy: true,
                  status: 'healthy',
                  allow_new_assignments: true,
                },
              ],
            },
          ],
          line_groups: [
            {
              id: 'group-smoke-1',
              name: 'AI 分组',
              enabled: true,
              sort_weight: 100,
              exit_endpoint_ids: ['exit-smoke-1'],
            },
          ],
        },
      }),
    });
  });
  await page.route('**/api/admin/exit-endpoints', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: [
          {
            id: 'exit-smoke-1',
            name: '出口管理出口 01',
            endpoint_name: '出口管理出口 01',
            resource_name: '自建线路资源',
            outbound_type: 'socks',
            host: 'upstream.example.test',
            port: 1080,
            enabled: true,
            exit_resource_enabled: true,
            healthy: true,
            status: 'healthy',
          },
        ],
      }),
    });
  });
  await page.route('**/api/admin/deployment-tasks', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          items: [
            {
              id: 'deploy-task-smoke-1',
              kind: 'agent_install',
              status: 'running',
              title: 'Agent 安装',
              summary: '服务器正在安装 access-agent',
              current_step: 'containers_started',
              progress_percent: 85,
              safe_metadata: {
                mode: 'one_click',
                access_node_name: 'Smoke 安装节点',
                public_host: 'install-smoke.example.test',
                public_port: 443,
                install_dir: '/opt/xrayc/access-agent',
                compose_project: 'xrayc-access',
                ssh_port: 22,
                ssh_auth_method: 'password',
                force_reinstall: true,
              },
              steps: [
                { key: 'containers_started', title: '启动运行组件', detail: '容器已启动', status: 'current' },
              ],
              result: {},
              error_summary: '',
              updated_at: '2026-06-03T00:00:00Z',
            },
          ],
        },
      }),
    });
  });

  await page.goto('/overview');

  await expect(page).toHaveURL(/\/overview$/);
  await expect(page.getByRole('heading', { name: '总览' })).toBeVisible();
  await expect(page.getByText('活跃用户')).toBeVisible();
  await expect(page.getByText('接入节点', { exact: true })).toBeVisible();
  await expectAdminWorkspaces(page);

  const accessRoutingResponse = page.waitForResponse(
    (response) => response.url().includes('/api/admin/access-routing') && response.request().method() === 'GET',
  );
  await page.goto('/admin/transit-nodes');
  const accessRoutingPayload = await (await accessRoutingResponse).json();
  const accessLines = Array.isArray(accessRoutingPayload?.data?.access_lines)
    ? accessRoutingPayload.data.access_lines
    : [];
  const hasNonPlaceholderAccessEndpoint = accessLines.some((line) => {
    const host = String(line?.listen_host ?? '');
    const port = Number(line?.listen_port ?? 0);
    return host.length > 0 && !host.includes('example') && !['0.0.0.0', '127.0.0.1', 'localhost'].includes(host) && port > 0;
  });
  if (requireRealAccessEndpoint && !hasNonPlaceholderAccessEndpoint) {
    throw new Error('access routing must include a non-placeholder listen endpoint');
  }
  await expect(page).toHaveURL(/\/admin\/transit-nodes$/);
  await expect(page.getByRole('heading', { name: '中转节点' })).toBeVisible();
  await expect(page.getByText('部署任务', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'install-smoke.example.test:443' })).toBeVisible();
  await expect(page.getByText('启动运行组件')).toBeVisible();
  if (requireRealAccessEndpoint) {
    await expect(page.getByText('access.example.test:443')).toHaveCount(0);
  }
  await expect(page.getByRole('button', { name: '新增中转节点', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '生成 Agent 安装说明', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: /刷新.*(入口|分配)/ })).toHaveCount(0);
  await expect(page.getByText(/运行入口|待同步|未创建/).first()).toBeVisible();
  await expect(page.getByText('客户端连接地址').first()).toBeVisible();
  await expect(page.getByText('出口线路').first()).toBeVisible();
  await expect(page.getByText('入站协议')).toHaveCount(0);
  await expect(page.getByText('Agent Token')).toHaveCount(0);
  await expect(page.getByText('SSH 地址')).toHaveCount(0);
  await expect(page.getByText('Xray API 端口')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Agent 安装说明', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '一键安装 Agent', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '选择出口管理线路', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '新增中转节点', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '一键部署', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '部署/重装', exact: true })).toHaveCount(0);
  await expectDialogOpensAndCloses(page, '新增中转节点', '新增中转节点');
  await openNodeOnboarding(page, 'manual');
  const installDialog = page.getByRole('dialog', { name: '手动安装 Agent' });
  await expect(installDialog).toBeVisible();
  await expect(installDialog.getByText('后台不会创建节点')).toBeVisible();
  await expect(installDialog.getByText('初始入口')).toHaveCount(0);
  await expect(installDialog.locator('.el-form-item').filter({ hasText: 'Agent Token' })).toHaveCount(0);
  await expect(installDialog.getByText('SSH 地址')).toHaveCount(0);
  await expect(installDialog.getByText('direct/local_direct')).toHaveCount(0);
  await installDialog.getByRole('button', { name: '关闭' }).click();
  await expect(installDialog).toBeHidden();
  await page.getByRole('button', { name: '本机出口服务', exact: true }).first().click();
  const localExitDialog = page.getByRole('dialog', { name: '本机出口服务' });
  await expect(localExitDialog).toBeVisible();
  await expect(localExitDialog.getByText('添加本机创建的出口线路到出口管理')).toBeVisible();
  await expect(localExitDialog.locator('.el-form-item').filter({ hasText: '线路协议' }).first().locator('.el-select')).toBeVisible();
  // 网络模式已拆为「传输（可多选）」多选 + 「承载（TCP/UDP）」（仅 RAW 传输出现承载）；传输下拉无条件渲染。
  await expect(localExitDialog.locator('.el-form-item').filter({ hasText: '传输（可多选）' }).first().locator('.el-select')).toBeVisible();
  // 该 smoke 节点只配了 IP 直连（无证书域名），本机出口协议护栏只放免证书协议
  // （VLESS Reality/none、Shadowsocks、SOCKS5、HTTP），要证书的 HY2/Trojan 被过滤掉。
  // 关闭态的 el-select 只把每行「已选中」的协议渲染成可见文本；该节点两条现有本机出口
  // 分别选了 VLESS 和 Shadowsocks，故只断言这两个可见，并断言 HY2/Trojan 不作为可选协议出现。
  await expect(localExitDialog.getByText('VLESS', { exact: true })).toBeVisible();
  await expect(localExitDialog.getByText('Shadowsocks', { exact: true })).toBeVisible();
  await expect(localExitDialog.getByText('HY2', { exact: true })).toHaveCount(0);
  await expect(localExitDialog.getByText('Trojan', { exact: true })).toHaveCount(0);
  await localExitDialog.getByRole('button', { name: '取消' }).click();
  await expect(localExitDialog).toBeHidden();
  await expect(page.getByRole('button', { name: '调整分组', exact: true })).toHaveCount(0);
  if (accessLines.length === 0) {
    await expect(page.getByText('该中转节点还没有运行入口').first()).toBeVisible();
  }
  await expect(page.getByRole('button', { name: '运行数据', exact: true })).toHaveCount(0);

  await page.goto('/admin/access-entries');
  await expect(page.getByRole('heading', { name: '入口管理' })).toBeVisible();
  await expect(page.getByRole('button', { name: '创建入口', exact: true })).toBeVisible();

  await page.route('**/api/admin/plans', async (route) => {
    if (route.request().method() !== 'GET') {
      await route.fallback();
      return;
    }

    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: [
          {
            id: '00000000-0000-0000-0000-000000000001',
            name: '基础套餐',
            is_default: true,
            enabled: true,
            traffic_limit_bytes: 10737418240,
            billing_multiplier: 1,
            price_cents: 0,
            currency: 'USDT',
            duration_days: 30,
            sort_weight: 1,
            line_groups: [{ line_group_id: 'demo-group', billing_multiplier: 1 }],
          },
          {
            id: '00000000-0000-0000-0000-000000000002',
            name: '标准套餐',
            is_default: false,
            enabled: true,
            traffic_limit_bytes: 107374182400,
            billing_multiplier: 1,
            price_cents: 990,
            currency: 'USDT',
            duration_days: 30,
            sort_weight: 10,
            line_groups: [{ line_group_id: 'demo-group', billing_multiplier: 1 }],
          },
        ],
      }),
    });
  });

  await page.goto('/admin/plans');
  await expect(page.getByRole('heading', { name: '套餐授权' })).toBeVisible();
  await expect(page.getByText('套餐列表')).toBeVisible();
  await expectDialogOpensAndCloses(page, '新增套餐', '新增套餐');
  await expectDialogOpensAndCloses(page, '编辑基础', /编辑套餐基础/);
  await page.getByRole('button', { name: '编辑分组', exact: true }).first().click();
  await expect(page.getByRole('dialog', { name: /配置套餐分组/ })).toBeVisible();
  await page.getByRole('dialog', { name: /配置套餐分组/ }).getByRole('button', { name: '取消' }).click();
  await expect(page.getByRole('row', { name: /基础套餐/ }).getByRole('button', { name: '删除' })).toBeDisabled();
  await expect(page.getByRole('row', { name: /标准套餐/ }).getByRole('button', { name: '删除' })).toBeEnabled();
  await expect(page.getByRole('row', { name: /标准套餐/ }).getByText('分组 demo-gro').first()).toBeVisible();

  await page.goto('/admin/line-groups');
  await expect(page.getByRole('heading', { name: '分组' })).toBeVisible();
  await expect(page.getByText('分组选择入口+出口绑定节点，可用于 AI、游戏、GPT、视频等不同用途。')).toBeVisible();
  await expect(page.getByText('分组内绑定节点')).toBeVisible();
  await expect(page.getByText('可选绑定节点')).toBeVisible();
  await expect(page.getByRole('button', { name: '创建分组', exact: true })).toBeVisible();
  await expectDialogOpensAndCloses(page, '创建分组', '创建分组');
  await expect(page.getByText('分配策略')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '重新分配线路', exact: true })).toHaveCount(0);

  await page.route(/.*\/api\/admin\/users(?:\?.*)?$/, async (route) => {
    if (route.request().method() !== 'GET') {
      await route.fallback();
      return;
    }

    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          items: [
            {
              id: 1001,
              account: 'demo@example.test',
              email: 'demo@example.test',
              name: 'Demo User',
              role: 'user',
              status: 'active',
              plan_id: '00000000-0000-0000-0000-000000000001',
              plan_name: '基础套餐',
              subscription_status: 'active',
              subscription_expires_at: '2026-06-01T00:00:00Z',
              traffic_gb: { used: 1, total: 10 },
              created_at: '2026-05-01T00:00:00Z',
            },
          ],
          total: 1,
          page: 1,
          page_size: 20,
        },
      }),
    });
  });
  await page.route('**/api/admin/users/1001', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          id: 1001,
          account: 'demo@example.test',
          email: 'demo@example.test',
          name: 'Demo User',
          role: 'user',
          status: 'active',
          plan_id: '00000000-0000-0000-0000-000000000001',
          plan_name: '基础套餐',
          subscription_status: 'active',
          subscription_expires_at: '2026-06-01T00:00:00Z',
          traffic_gb: { used: 1, total: 10 },
          created_at: '2026-05-01T00:00:00Z',
        },
      }),
    });
  });

  await page.goto('/admin/users');
  await expect(page.getByRole('heading', { name: '用户管理' })).toBeVisible();
  await page.getByRole('button', { name: '编辑用户', exact: true }).first().click();
  const userEditDialog = page.getByRole('dialog', { name: /编辑用户/ });
  await expect(userEditDialog).toBeVisible();
  await expect(userEditDialog.locator('.el-form-item').filter({ hasText: '邮箱' }).locator('input')).toBeVisible();
  await expect(userEditDialog.getByText('启用', { exact: true })).toBeVisible();
  await expect(userEditDialog.getByText('管理员', { exact: true })).toBeVisible();
  await expect(userEditDialog.locator('.el-form-item').filter({ hasText: '套餐' }).locator('.el-select')).toBeVisible();
  await userEditDialog.getByRole('button', { name: '取消' }).click();
  await expect(userEditDialog).toBeHidden();

  await page.route('**/api/admin/audit-logs', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: [
          {
            id: 'audit-1',
            created_at: '2026-05-19T00:00:00Z',
            actor_account: 'admin',
            action: 'access_node.deploy',
            resource_type: 'access_node',
            resource_id: 'node-1',
            result: 'succeeded',
            request_summary: {
              operation: '部署接入节点',
              safe_note: '管理员查看完整请求摘要',
              ssh_password: 'super-secret-password',
              agent_token: 'secret-agent-token',
              subscription_url: 'https://panel.example.test/sub/full-token',
              outbound_config: {
                protocol: 'vless',
                host: 'proxy.example.test',
                port: 443,
              },
            },
          },
        ],
      }),
    });
  });

  await page.goto('/admin/audit-logs');
  await expect(page.getByRole('heading', { name: '审计日志' })).toBeVisible();
  await expect(page.getByRole('link', { name: '操作审计', exact: true })).toBeVisible();
  await expect(page.getByRole('main').getByText('admin')).toBeVisible();
  await expect(page.getByText('access_node.deploy')).toBeVisible();
  await expect(page.getByText('access_node:node-1')).toBeVisible();
  await expect(page.getByText('管理员查看完整请求摘要')).toBeVisible();
  await expect(page.getByText('super-secret-password')).toBeVisible();
  await expect(page.getByText('secret-agent-token')).toBeVisible();
  await expect(page.getByText('/sub/full-token')).toBeVisible();
  await expect(page.getByText('proxy.example.test')).toBeVisible();

  await page.route('**/api/plans', async (route) => {
    if (route.request().method() !== 'GET') {
      await route.fallback();
      return;
    }

    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: [
          {
            id: '00000000-0000-0000-0000-000000000002',
            name: '标准套餐',
            is_default: false,
            enabled: true,
            traffic_limit_bytes: 107374182400,
            billing_multiplier: 1,
            price_cents: 990,
            currency: 'USDT',
            duration_days: 30,
            sort_weight: 10,
            line_groups: [],
          },
        ],
      }),
    });
  });
  await page.route('**/api/admin/redeem-codes', async (route) => {
    if (route.request().method() !== 'GET') {
      await route.fallback();
      return;
    }

    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          items: [],
        },
      }),
    });
  });
  const pendingAdminPages = [
    { path: '/admin/orders', heading: '订单管理' },
    { path: '/admin/redeem-codes', heading: '兑换码管理' },
    { path: '/admin/auth-security', heading: '认证安全' },
    { path: '/admin/subscription-settings', heading: '订阅设置' },
    { path: '/admin/sales-landing', heading: '销售首页配置' },
  ];

  for (const adminPage of pendingAdminPages) {
    await page.goto(adminPage.path);
    await expect(page.getByRole('heading', { name: adminPage.heading })).toBeVisible();
    await expect(page.getByText('后端接口待接入')).toHaveCount(0);

    if (adminPage.path === '/admin/redeem-codes') {
      await page.getByRole('button', { name: '批量生成' }).click();
      const redeemDialog = page.getByRole('dialog', { name: '批量生成兑换码' });
      await expect(redeemDialog).toBeVisible();
      await expect(redeemDialog.getByText('每次最多 100 个，与后端限制一致')).toBeVisible();
      await redeemDialog.getByRole('button', { name: '取消' }).click();
      await expect(redeemDialog).toBeHidden();
    }
  }
});
