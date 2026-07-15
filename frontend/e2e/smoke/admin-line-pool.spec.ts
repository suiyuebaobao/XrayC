// 用途：覆盖后台出口管理页面，确保管理员只看到统一线路列表。
// 测试数据只使用 example.test 与 198.51.100.0/24 保留地址。
import { expect, test, type Page, type Request, type Route } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员在出口管理查看统一线路列表', async ({ page }) => {
  await mockAdminSession(page);
  await mockLinePoolApis(page);

  await page.goto('/admin/line-pool');
  await expect(page.getByRole('heading', { name: '出口管理' })).toBeVisible();
  await expect(page.getByText('出口管理（统一列表）', { exact: true })).toBeVisible();

  const summary = page.locator('.line-pool-summary');
  await expect(summary.locator('.el-card').filter({ has: page.locator('span', { hasText: /^线路$/ }) })).toContainText('2');
  await expect(summary.locator('.el-card', { hasText: '可用线路' })).toContainText('2');

  await expect(page.getByText('用户入口')).toHaveCount(0);
  await expect(page.getByText('线路归属')).toHaveCount(0);
  await expect(page.getByText('第三方线路')).toHaveCount(0);
  await expect(page.getByText('中转本机出口服务')).toHaveCount(0);
  await expect(page.getByText('Example VLESS Exit', { exact: true })).toBeVisible();
  await expect(page.getByText('edge-vless.example.test:443')).toBeVisible();
  await expect(page.getByText('Example SOCKS Exit', { exact: true })).toBeVisible();
  await expect(page.getByText('198.51.100.42:1080')).toBeVisible();
  await expect(page.getByText('外部')).toBeVisible();
  await expect(page.getByText('自己创建 / 香港中转 01')).toBeVisible();
});

test('管理员粘贴 VLESS 配置后自动识别并预填新增线路表单', async ({ page }) => {
  await mockAdminSession(page);
  const apiMock = await mockLinePoolApis(page);

  await page.goto('/admin/line-pool');
  await page.getByRole('button', { name: '添加线路' }).click();
  const dialog = page.getByRole('dialog', { name: '添加线路' });
  await expect(dialog).toBeVisible();

  await dialog.getByLabel('原始线路配置').fill(`
name: 'Example-Reality-T1', type: vless, server: vless-reality.example.test, port: 443, uuid: 00000000-0000-4000-8000-000000000123, network: tcp, tls: true, udp: true, flow: xtls-rprx-vision, client-fingerprint: chrome, servername: reality-sni.example.test, reality-opts: { public-key: exampleRealityPublicKeyForSmokeOnly000000000, short-id: 1234abcd }
`);

  await expect(dialog.getByText('已自动识别 1 条线路')).toBeVisible();
  await expect(dialog.getByLabel('线路名称')).toHaveValue('Example-Reality-T1');
  await expect(dialog.locator('.el-form-item').filter({ hasText: '协议' })).toContainText('VLESS');
  await expect(dialog.getByLabel('地址')).toHaveValue('vless-reality.example.test');
  await expect(dialog.locator('.el-form-item').filter({ hasText: '端口' }).locator('input')).toHaveValue('443');
  await expect(dialog.locator('.el-form-item').filter({ hasText: '连接配置 JSON' }).locator('textarea')).toHaveValue(
    /"security": "reality"/,
  );
  await expect(dialog.locator('.el-form-item').filter({ hasText: '连接配置 JSON' }).locator('textarea')).toHaveValue(
    /"flow": "xtls-rprx-vision"/,
  );
  await expect(dialog.locator('.el-form-item').filter({ hasText: 'Stream 配置 JSON' }).locator('textarea')).toHaveValue(
    /"network": "tcp"/,
  );

  const resourceCreate = page.waitForRequest((request) => isRequest(request, 'POST', '/api/admin/exit-resources'));
  const endpointCreate = page.waitForRequest((request) => isRequest(request, 'POST', '/api/admin/exit-endpoints'));
  await dialog.getByRole('button', { name: '保存' }).click();
  await resourceCreate;
  await endpointCreate;
  await expect(dialog).toBeHidden();
  expect(apiMock.createdResources).toHaveLength(1);
  expect(apiMock.createdEndpoints).toHaveLength(1);
  expect(apiMock.createdEndpoints[0]).toMatchObject({
    name: 'Example-Reality-T1',
    outbound_type: 'vless',
    host: 'vless-reality.example.test',
    port: 443,
    stream_config: { network: 'tcp' },
  });
});

type JsonRecord = Record<string, unknown>;

type LinePoolApiMock = {
  createdResources: JsonRecord[];
  createdEndpoints: JsonRecord[];
  resourceUpdates: Array<{ id: string; payload: JsonRecord }>;
  endpointUpdates: Array<{ id: string; payload: JsonRecord }>;
  probeRequests: string[];
  deleteRequests: string[];
  endpointListRequests: number;
};

async function mockLinePoolApis(page: Page): Promise<LinePoolApiMock> {
  const apiMock: LinePoolApiMock = {
    createdResources: [],
    createdEndpoints: [],
    resourceUpdates: [],
    endpointUpdates: [],
    probeRequests: [],
    deleteRequests: [],
    endpointListRequests: 0,
  };
  const accessLines = [
    {
      id: 1,
      uuid: 'access-line-1',
      name: 'Example Access VLESS',
      region: 'US',
      access_node_name: 'Example Access Node',
      listen_host: 'access-us.example.test',
      listen_port: 443,
      protocol: 'vless',
      status: 'enabled',
      exit_pool_name: 'Example Pool',
      exit_pool_id: 'pool-example',
      probe_status: 'healthy',
      probe_latency_ms: 82,
    },
    {
      id: 2,
      uuid: 'access-line-2',
      name: 'Example Access Hidden',
      region: 'US',
      access_node_name: 'Example Access Node',
      listen_host: 'access-hidden.example.test',
      listen_port: 8443,
      protocol: 'vless',
      status: 'disabled',
      exit_pool_name: 'Example Pool',
      exit_pool_id: 'pool-example',
      probe_status: 'healthy',
      probe_latency_ms: 95,
    },
  ];
  const lineGroups = [
    {
      id: 'group-visible',
      name: 'Example Visible Group',
      enabled: true,
      exit_endpoint_ids: ['1', '2'],
    },
    {
      id: 'group-hidden',
      name: 'Example Hidden Group',
      enabled: false,
      exit_endpoint_ids: [],
    },
  ];
  let exitResources = [
    {
      id: 'resource-vless-1',
      name: 'Example VLESS Exit',
      region_code: 'US',
      provider_name: 'Example Carrier',
      ownership: 'third_party',
      enabled: true,
      created_at: '2026-05-26T00:00:00Z',
    },
    {
      id: 'resource-socks-1',
      name: 'Example SOCKS Exit',
      region_code: 'TEST-NET',
      provider_name: 'Example Lab',
      ownership: 'self_hosted',
      access_node_id: 'node-relay-1',
      access_node_name: '香港中转 01',
      enabled: true,
      created_at: '2026-05-26T00:00:00Z',
    },
  ];
  let exitEndpoints = [
    {
      id: 'endpoint-vless-1',
      exit_resource_id: 'resource-vless-1',
      exit_resource_name: 'Example VLESS Exit',
      exit_resource_enabled: true,
      name: 'Example VLESS Exit',
      outbound_type: 'vless',
      host: 'edge-vless.example.test',
      host_redacted: false,
      port: 443,
      outbound_config: {
        uuid: '00000000-0000-4000-8000-000000000111',
        public_key: 'exampleRealityPublicKeyForSmokeOnly000000111',
        short_id: 'abcd1111',
        security: 'reality',
        server_name: 'edge-vless.example.test',
        flow: 'xtls-rprx-vision',
      },
      stream_config: { network: 'tcp' },
      probe_config: { tcp_timeout_ms: 3000 },
      enabled: true,
      last_probe_status: 'healthy',
      created_at: '2026-05-26T00:00:00Z',
    },
    {
      id: 'endpoint-socks-1',
      exit_resource_id: 'resource-socks-1',
      exit_resource_name: 'Example SOCKS Exit',
      exit_resource_enabled: true,
      name: 'Example SOCKS Exit',
      outbound_type: 'socks',
      host: '198.51.100.42',
      host_redacted: false,
      port: 1080,
      outbound_config: {
        username: 'example-socks-user',
        password: 'example-socks-credential',
      },
      stream_config: {},
      probe_config: { tcp_timeout_ms: 3000 },
      enabled: true,
      last_probe_status: 'healthy',
      created_at: '2026-05-26T00:00:00Z',
    },
  ];

  await page.route('**/api/admin/access-routing', async (route) => {
    await fulfillJson(route, {
      success: true,
      data: {
        access_lines: accessLines,
        line_groups: lineGroups,
        exit_pools: [],
        access_nodes: [],
      },
    });
  });

  await page.route('**/api/admin/exit-resources**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const method = request.method();

    if (method === 'GET' && url.pathname === '/api/admin/exit-resources') {
      await fulfillJson(route, { success: true, data: { items: exitResources } });
      return;
    }

    if (method === 'POST' && url.pathname === '/api/admin/exit-resources') {
      const payload = requestJson(request);
      const id = `created-resource-${apiMock.createdResources.length + 1}`;
      apiMock.createdResources.push(payload);
      exitResources = [
        ...exitResources,
        {
          id,
          name: stringPayload(payload.name),
          region_code: stringPayload(payload.region_code),
          provider_name: stringPayload(payload.provider_name),
          ownership: stringPayload(payload.ownership) || 'third_party',
          enabled: payload.enabled !== false,
          created_at: '2026-05-26T00:00:00Z',
        },
      ];
      await fulfillJson(route, { success: true, data: { id } });
      return;
    }

    const resourceUpdateMatch = url.pathname.match(/^\/api\/admin\/exit-resources\/([^/]+)$/);
    if (method === 'PUT' && resourceUpdateMatch) {
      const id = resourceUpdateMatch[1];
      const payload = requestJson(request);
      apiMock.resourceUpdates.push({ id, payload });
      exitResources = exitResources.map((resource) => (
        resource.id === id
          ? {
              ...resource,
              name: stringPayload(payload.name) || resource.name,
              region_code: stringPayload(payload.region_code) || resource.region_code,
              provider_name: stringPayload(payload.provider_name) || resource.provider_name,
              ownership: stringPayload(payload.ownership) || resource.ownership,
              enabled: payload.enabled !== false,
            }
          : resource
      ));
      await fulfillJson(route, { success: true, data: {} });
      return;
    }

    await unhandledRoute(route);
  });

  await page.route('**/api/admin/exit-endpoints**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const method = request.method();

    if (method === 'GET' && url.pathname === '/api/admin/exit-endpoints') {
      apiMock.endpointListRequests += 1;
      await fulfillJson(route, { success: true, data: { items: exitEndpoints } });
      return;
    }

    if (method === 'POST' && url.pathname === '/api/admin/exit-endpoints') {
      const payload = requestJson(request);
      const id = `created-endpoint-${apiMock.createdEndpoints.length + 1}`;
      apiMock.createdEndpoints.push(payload);
      exitEndpoints = [
        ...exitEndpoints,
        {
          id,
          exit_resource_id: stringPayload(payload.exit_resource_id),
          exit_resource_name: stringPayload(payload.name),
          exit_resource_enabled: true,
          name: stringPayload(payload.name),
          outbound_type: stringPayload(payload.outbound_type),
          host: stringPayload(payload.host),
          host_redacted: false,
          port: numberPayload(payload.port),
          outbound_config: recordPayload(payload.outbound_config),
          stream_config: recordPayload(payload.stream_config),
          probe_config: recordPayload(payload.probe_config),
          enabled: payload.enabled !== false,
          last_probe_status: 'unknown',
          created_at: '2026-05-26T00:00:00Z',
        },
      ];
      await fulfillJson(route, { success: true, data: { id } });
      return;
    }

    const probeMatch = url.pathname.match(/^\/api\/admin\/exit-endpoints\/([^/]+)\/probe$/);
    if (method === 'POST' && probeMatch) {
      apiMock.probeRequests.push(probeMatch[1]);
      await fulfillJson(route, { success: true, data: {} });
      return;
    }

    const endpointMatch = url.pathname.match(/^\/api\/admin\/exit-endpoints\/([^/]+)$/);
    if (method === 'PUT' && endpointMatch) {
      const id = endpointMatch[1];
      const payload = requestJson(request);
      apiMock.endpointUpdates.push({ id, payload });
      exitEndpoints = exitEndpoints.map((endpoint) => (
        endpoint.id === id
          ? {
              ...endpoint,
              exit_resource_id: stringPayload(payload.exit_resource_id) || endpoint.exit_resource_id,
              exit_resource_name: stringPayload(payload.name) || endpoint.exit_resource_name,
              name: stringPayload(payload.name) || endpoint.name,
              outbound_type: stringPayload(payload.outbound_type) || endpoint.outbound_type,
              host: stringPayload(payload.host) || endpoint.host,
              port: numberPayload(payload.port) || endpoint.port,
              outbound_config: recordPayload(payload.outbound_config),
              stream_config: recordPayload(payload.stream_config),
              probe_config: recordPayload(payload.probe_config),
              enabled: payload.enabled !== false,
            }
          : endpoint
      ));
      await fulfillJson(route, { success: true, data: {} });
      return;
    }

    if (method === 'DELETE' && endpointMatch) {
      apiMock.deleteRequests.push(endpointMatch[1]);
      exitEndpoints = exitEndpoints.filter((endpoint) => endpoint.id !== endpointMatch[1]);
      await fulfillJson(route, { success: true, data: {} });
      return;
    }

    await unhandledRoute(route);
  });

  return apiMock;
}

function isRequest(request: Request, method: string, pathname: string) {
  return request.method() === method && new URL(request.url()).pathname === pathname;
}

function requestJson(request: Request): JsonRecord {
  return JSON.parse(request.postData() || '{}') as JsonRecord;
}

async function fulfillJson(route: Route, body: JsonRecord, status = 200) {
  await route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  });
}

async function unhandledRoute(route: Route) {
  const request = route.request();
  await fulfillJson(route, {
    success: false,
    message: `Unhandled mock route: ${request.method()} ${new URL(request.url()).pathname}`,
  }, 500);
}

function stringPayload(value: unknown) {
  return typeof value === 'string' ? value : '';
}

function numberPayload(value: unknown) {
  return typeof value === 'number' ? value : Number(value || 0);
}

function recordPayload(value: unknown) {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as JsonRecord : {};
}
