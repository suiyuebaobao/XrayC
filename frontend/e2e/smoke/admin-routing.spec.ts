// 用途：覆盖管理员按绑定节点创建分组，入口绑定由入口管理承载。
import { expect, type Route, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员按绑定节点创建分组并检查中转节点运行入口路径', async ({ page }) => {
  await mockAdminSession(page);

  const state = {
    pools: [
      {
        id: 'pool-existing',
        uuid: 'pool-existing',
        name: '已建美国出口池',
        region: 'US',
        status: 'healthy',
        healthy_members: 1,
        total_members: 1,
        strategy: 'priority',
        active_assignments: 0,
        members: [
          {
            id: 'exit-existing-1',
            name: '已建上游出口',
            endpoint_name: '已建上游出口',
            resource_name: '已建上游出口',
            outbound_type: 'socks',
            host: 'upstream.example.test',
            port: 1080,
            outbound_config: {
              username: 'pool-user',
              ['pass' + 'word']: 'pool-password',
            },
            healthy: true,
            status: 'healthy',
            status_known: true,
            weight: 100,
            priority: 100,
            allow_new_assignments: true,
          },
        ],
      },
    ],
    lines: [] as Array<Record<string, unknown>>,
    entries: [
      {
        id: 'entry-hk-vless',
        access_node_id: 'node-relay-1',
        access_node_name: '香港中转 01',
        name: '香港入口',
        listen_host: 'hk-relay.example.test',
        listen_port: 443,
        protocol: 'vless',
        transport: 'tcp',
        security: 'reality',
        enabled: true,
        sort_weight: 100,
      },
    ],
    bindings: [
      {
        id: 'binding-existing-1',
        access_entry_id: 'entry-hk-vless',
        access_entry_name: '香港入口',
        access_node_id: 'node-relay-1',
        access_node_name: '香港中转 01',
        exit_endpoint_id: 'exit-existing-1',
        exit_endpoint_name: '已建上游出口',
        exit_pool_id: 'pool-existing',
        exit_pool_name: '出口线路：已建上游出口',
        name: '香港入口 + 已建上游出口',
        enabled: true,
        sort_weight: 100,
      },
    ],
    lineGroups: [
      {
        id: 'line-group-default',
        name: '默认分组',
        enabled: true,
        sort_weight: 100,
        binding_node_ids: ['binding-existing-1'] as string[],
        rule_set_bindings: [
          {
            rule_set_id: 'rule-set-ai',
            rule_set_name: 'AI 直连规则库',
            position: 100,
            enabled: true,
          },
        ],
      },
    ],
    ruleSets: [
      {
        id: 'rule-set-ai',
        name: 'AI 直连规则库',
        description: 'AI 域名走当前分组',
        enabled: true,
        rules: ['DOMAIN-SUFFIX,openai.com,PROXY'],
        sort_weight: 100,
        binding_count: 1,
      },
    ],
  };
  let createLineGroupPayload: Record<string, unknown> | null = null;
  let replaceLineGroupBindingNodesPayload: Record<string, unknown> | null = null;
  let directLineGroupLineRequestCount = 0;
  let remoteDeployRequestCount = 0;
  let tlsRenewRequestCount = 0;

  await page.route('**/api/admin/exit-pools', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: state.pools }),
    });
  });
  await page.route('**/api/admin/exit-endpoints', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: state.pools.flatMap((pool) => pool.members),
      }),
    });
  });
  await page.route('**/api/admin/access-entries', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { entries: state.entries } }),
    });
  });
  await page.route('**/api/admin/access-entry-exit-bindings', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { bindings: state.bindings } }),
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
              id: 'node-relay-1',
              name: '香港中转 01',
              public_host: 'hk-relay.example.test',
              config_dirty: false,
              tls_certificates: [
                {
                  domain: 'hk-relay.example.test',
                  status: 'expiring',
                  not_after: '2026-06-20T00:00:00Z',
                  days_remaining: 9,
                  error_summary: '',
                },
              ],
              tls_renew_status: tlsRenewRequestCount > 0 ? 'queued' : '',
              tls_renew_message: tlsRenewRequestCount > 0 ? '等待节点执行 SSL 证书续期' : '',
            },
          ],
          access_lines: state.lines,
          exit_pools: state.pools,
          line_groups: state.lineGroups,
        },
      }),
    });
  });
  await page.route('**/api/admin/deployment-tasks', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { items: [] } }),
    });
  });
  await page.route('**/api/admin/access-nodes/node-relay-1/tls/renew', async (route) => {
    if (route.request().method() !== 'POST') {
      await route.fallback();
      return;
    }
    tlsRenewRequestCount += 1;
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          request_id: '00000000-0000-0000-0000-000000000901',
          status: 'queued',
          domains: ['hk-relay.example.test'],
        },
      }),
    });
  });
  await page.route('**/api/admin/line-groups', async (route) => {
    if (route.request().method() !== 'POST') {
      await route.fallback();
      return;
    }

    createLineGroupPayload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
    state.lineGroups.push({
      id: 'line-group-new',
      name: String(createLineGroupPayload.name ?? ''),
      enabled: createLineGroupPayload.enabled !== false,
      sort_weight: Number(createLineGroupPayload.sort_weight ?? 100),
      binding_node_ids: [],
      rule_set_bindings: Array.isArray(createLineGroupPayload.rule_set_bindings)
        ? createLineGroupPayload.rule_set_bindings.map((binding, index) => {
            const data = binding as Record<string, unknown>;
            const ruleSetId = String(data.rule_set_id ?? '');
            const ruleSet = state.ruleSets.find((item) => item.id === ruleSetId);
            return {
              rule_set_id: ruleSetId,
              rule_set_name: ruleSet?.name ?? '',
              position: Number(data.position ?? (index + 1) * 100),
              enabled: data.enabled !== false,
            };
          })
        : [],
    });
    await route.fulfill({
      status: 201,
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { id: 'line-group-new' } }),
    });
  });
  await page.route('**/api/admin/line-groups/*/lines', async (route) => {
    directLineGroupLineRequestCount += 1;
    await route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ success: false, message: 'line groups must bind access-entry exit binding nodes' }),
    });
  });
  await page.route('**/api/admin/line-groups/*/binding-nodes', async (route) => {
    replaceLineGroupBindingNodesPayload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
    const group = state.lineGroups.find((lineGroup) => route.request().url().includes(lineGroup.id));
    if (group) {
      const bindingNodeIds = Array.isArray(replaceLineGroupBindingNodesPayload.binding_node_ids)
        ? replaceLineGroupBindingNodesPayload.binding_node_ids
        : [];
      group.binding_node_ids = bindingNodeIds.map(String);
    }
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: {} }),
    });
  });
  await page.route('**/api/admin/subscription-rule-sets', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          rule_sets: state.ruleSets,
        },
      }),
    });
  });
  await page.route('**/api/admin/plans', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: [
          {
            id: 'plan-default',
            name: '基础套餐',
            is_default: true,
            enabled: true,
            traffic_limit_bytes: 10737418240,
            billing_multiplier: 1,
            price_cents: 0,
            currency: 'USDT',
            duration_days: 30,
            sort_weight: 1,
            line_groups: [{ line_group_id: 'line-group-default', billing_multiplier: 1 }],
          },
        ],
      }),
    });
  });
  const failUnexpectedDeploy = async (route: Route) => {
    remoteDeployRequestCount += 1;
    await route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ success: false, message: 'deploy must not be called in this smoke test' }),
    });
  };
  await page.route('**/api/admin/access-nodes/deploy', failUnexpectedDeploy);
  await page.route('**/api/admin/access-node-deploy', failUnexpectedDeploy);

  await page.goto('/admin/line-groups');
  await expect(page.getByRole('heading', { name: '分组' })).toBeVisible();
  await expect(page.getByText('分组选择入口+出口绑定节点，可用于 AI、游戏、GPT、视频等不同用途。')).toBeVisible();
  await expect(page.getByText('分组内绑定节点')).toBeVisible();
  await expect(page.getByText('可选绑定节点')).toBeVisible();
  await expect(page.getByText('默认分组')).toBeVisible();
  await expect(page.getByText('AI 直连规则库')).toBeVisible();
  await expect(page.getByText('已建美国出口池')).toHaveCount(0);
  await page.getByRole('button', { name: '创建分组', exact: true }).click();
  const createEmptyGroupDialog = page.getByRole('dialog', { name: '创建分组' });
  await expect(createEmptyGroupDialog).toBeVisible();
  await expect(createEmptyGroupDialog.locator('.el-form-item').filter({ hasText: '选择绑定节点' }).locator('.el-select')).toBeVisible();
  await createEmptyGroupDialog.getByRole('button', { name: '取消' }).click();
  await expect(createEmptyGroupDialog).toBeHidden();

  await page.goto('/admin/transit-nodes');
  await expect(page.getByRole('heading', { name: '中转节点' })).toBeVisible();
  await expect(page.getByText(/登记中转节点，生成 Agent 安装说明/)).toBeVisible();
  await expect(page.getByText('该中转节点还没有运行入口')).toBeVisible();
  await expect(page.getByText('SSL：hk-relay.example.test')).toBeVisible();
  await expect(page.getByText('9 天后到期')).toBeVisible();
  await page.getByRole('button', { name: '续期 SSL', exact: true }).click();
  await expect(page.getByText('等待节点执行 SSL 证书续期')).toBeVisible();
  expect(tlsRenewRequestCount).toBe(1);
  await expect(page.getByText('可用线路')).toBeVisible();
  await expect(page.getByText('1 / 1')).toBeVisible();
  await expect(page.getByText('日本备用出口池')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '新增中转节点', exact: true })).toBeVisible();
  await page.goto('/admin/access-entries');
  await expect(page.getByRole('heading', { name: '入口管理' })).toBeVisible();
  await expect(page.getByRole('button', { name: '创建入口', exact: true })).toBeVisible();
  await page.goto('/admin/line-groups');
  await page.getByRole('button', { name: '创建分组', exact: true }).click();
  const boundSecondaryDialog = page.getByRole('dialog', { name: '创建分组' });
  await boundSecondaryDialog.locator('.el-form-item').filter({ hasText: '分组名称' }).locator('input').fill('美国线路');
  await boundSecondaryDialog.locator('.el-form-item').filter({ hasText: '绑定规则库' }).locator('.el-select').click();
  await page.locator('.el-select-dropdown__item').filter({ hasText: 'AI 直连规则库' }).click();
  await page.keyboard.press('Escape');
  await boundSecondaryDialog.locator('.el-form-item').filter({ hasText: '选择绑定节点' }).locator('.el-select').click();
  await page.locator('.el-select-dropdown__item').filter({ hasText: '香港入口 + 已建上游出口' }).click();
  await page.keyboard.press('Escape');
  await boundSecondaryDialog.getByRole('button', { name: '保存' }).click();
  await expect(boundSecondaryDialog).toBeHidden();
  expect(createLineGroupPayload).toMatchObject({ name: '美国线路' });
  expect(createLineGroupPayload).toMatchObject({
    rule_set_bindings: [
      {
        rule_set_id: 'rule-set-ai',
        position: 100,
        enabled: true,
      },
    ],
  });
  expect(createLineGroupPayload).not.toHaveProperty('group_level');
  expect(createLineGroupPayload).not.toHaveProperty('parent_group_id');
  expect(createLineGroupPayload).not.toHaveProperty('icon');
  expect(createLineGroupPayload).not.toHaveProperty('dedicated_rules');
  expect(replaceLineGroupBindingNodesPayload).toMatchObject({
    binding_node_ids: ['binding-existing-1'],
  });
  expect(directLineGroupLineRequestCount).toBe(0);
  expect(remoteDeployRequestCount).toBe(0);
});
