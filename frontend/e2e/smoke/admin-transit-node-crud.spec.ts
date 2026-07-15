// 用途：覆盖中转节点编辑、批量删除和本机出口服务弹窗。
// 本用例使用 Playwright 路由 mock，只验证前端动作和 API 契约。
// 真实链路仍由脚本和远端服务器 E2E 覆盖。

import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员可以创建一键安装 Agent 任务', async ({ page }) => {
  await mockAdminSession(page);

  const deploymentTasks: Array<Record<string, unknown>> = [];
  let oneClickPayload: Record<string, unknown> | null = null;

  await page.route('**/api/admin/access-routing', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_nodes: [],
          access_lines: [],
          exit_pools: [],
          line_groups: [],
        },
      }),
    });
  });
  await page.route('**/api/admin/exit-endpoints', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: [] }),
    });
  });
  await page.route('**/api/admin/deployment-tasks', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { items: deploymentTasks } }),
    });
  });
  await page.route('**/api/admin/access-nodes/one-click-install', async (route) => {
    oneClickPayload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
    const task = {
      id: 'deploy-task-one-click-1',
      kind: 'agent_install',
      status: 'running',
      title: 'Agent 安装',
      summary: '服务器正在安装 access-agent',
      current_step: 'ssh_connect',
      progress_percent: 20,
      safe_metadata: {
        mode: 'one_click',
        access_node_name: '新加坡一键节点',
        public_host: 'agent.example.test',
        public_port: 443,
        install_dir: '/opt/xrayc/access-agent',
        compose_project: 'xrayc-access',
        ssh_port: 22,
        ssh_auth_method: 'password',
        tls_cert_domain_count: 1,
        tls_cert_email_present: true,
        force_reinstall: true,
      },
      steps: [
        { key: 'one_click_requested', title: '创建安装任务', detail: '已接收一键安装请求', status: 'done' },
        { key: 'ssh_connect', title: '连接服务器', detail: '平台正在连接 SSH 并执行远端安装', status: 'current' },
      ],
      result: {},
      error_summary: '',
      updated_at: '2026-06-14T00:00:00Z',
    };
    deploymentTasks.splice(0, deploymentTasks.length, task);
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          status: 'queued',
          access_node_id: 'node-one-click-1',
          task,
        },
      }),
    });
  });

  await page.goto('/admin/transit-nodes');
  await page.getByRole('button', { name: '一键安装 Agent' }).click();
  const dialog = page.getByRole('dialog', { name: '一键安装 Agent' });
  await expect(dialog).toBeVisible();

  await dialog.locator('.el-form-item').filter({ hasText: '节点名称' }).locator('input').fill('新加坡一键节点');
  // 三地址自动推导（2026-06）：弹窗已无独立「公网地址」字段，public_host 由 IP直连>域名直连>CF 推导；
  // 这里只填域名直连地址（下方 line 101），public_host 即推导为该域名 agent.example.test。
  await dialog.locator('.el-form-item').filter({ hasText: '公网端口' }).locator('input').fill('443');
  await dialog.locator('.el-form-item').filter({ hasText: '备注' }).locator('textarea').fill('created from frontend one-click flow');
  await dialog.locator('.el-form-item').filter({ hasText: 'SSH Host' }).locator('input').fill('192.0.2.10');
  await dialog.locator('.el-form-item').filter({ hasText: 'SSH Port' }).locator('input').fill('22');
  await dialog.locator('.el-form-item').filter({ hasText: 'SSH User' }).locator('input').fill('root');
  await dialog.locator('.el-form-item').filter({ hasText: 'SSH 密码' }).locator('input').fill('ssh-password-never-log');
  await dialog.locator('.el-form-item').filter({ hasText: '安装目录' }).locator('input').fill('/opt/xrayc/access-agent');
  await dialog.locator('.el-form-item').filter({ hasText: 'Compose Project' }).locator('input').fill('xrayc-access');
  await dialog.locator('.el-form-item').filter({ hasText: '域名直连地址' }).locator('input').fill('agent.example.test');
  await dialog.locator('.el-form-item').filter({ hasText: 'ACME 邮箱' }).locator('input').fill('ops@example.test');

  await Promise.all([
    page.waitForRequest((request) =>
      request.url().endsWith('/api/admin/access-nodes/one-click-install') && request.method() === 'POST',
    ),
    dialog.getByRole('button', { name: '创建安装任务' }).click(),
  ]);

  const sshPass = ['ssh', 'password', 'never', 'log'].join('-');
  expect(oneClickPayload).toMatchObject({
    name: '新加坡一键节点',
    public_host: 'agent.example.test',
    public_port: 443,
    remark: 'created from frontend one-click flow',
    ssh_host: '192.0.2.10',
    ssh_port: 22,
    ssh_user: 'root',
    ssh_password: sshPass,
    install_dir: '/opt/xrayc/access-agent',
    compose_project: 'xrayc-access',
    tls_cert_domains: ['agent.example.test'],
    acme_email: 'ops@example.test',
    force_reinstall: true,
  });
  // 控制面地址不再由前端下发：浏览器 origin 在本机/IP/备用域名访问时远端 agent 不可达，
  // 故前端留空、由后端按稳定中心地址 XRAYC_PUBLIC_BASE_URL 注入，payload 不含 control_plane_url。
  expect(oneClickPayload?.control_plane_url).toBeFalsy();
  await expect(dialog).toBeHidden();
  await expect(page.getByText('安装任务已创建')).toBeVisible();
  await expect(page.getByText('agent.example.test:443')).toBeVisible();
  await expect(page.getByText('连接服务器', { exact: true })).toBeVisible();
});

test('部署任务按服务器地址聚合并支持失败重试预填', async ({ page }) => {
  await mockAdminSession(page);

  await page.route('**/api/admin/access-routing', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_nodes: [],
          access_lines: [],
          exit_pools: [],
          line_groups: [],
        },
      }),
    });
  });
  await page.route('**/api/admin/exit-endpoints', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: [] }),
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
              id: 'deploy-task-failed-new',
              kind: 'agent_install',
              status: 'failed',
              title: 'Agent 一键安装',
              summary: '远程安装命令执行失败',
              current_step: 'ssh_install_failed',
              progress_percent: 100,
              safe_metadata: {
                mode: 'one_click',
                access_node_name: '新加坡一键节点',
                public_host: 'agent.example.test',
                public_port: 443,
                // SSH IP 是与客户连接地址（public_host）无关的独立字段，失败重试只回填它本身。
                ssh_host: '192.0.2.10',
                install_dir: '/opt/xrayc/access-agent',
                compose_project: 'xrayc-access',
                ssh_port: 22,
                ssh_auth_method: 'password',
                force_reinstall: true,
              },
              steps: [
                { key: 'one_click_requested', title: '创建安装任务', detail: '已接收一键安装请求，后续由平台自动执行', status: 'done' },
                { key: 'ssh_connect', title: '连接服务器', detail: '已连接服务器', status: 'done' },
                { key: 'ssh_install_failed', title: 'SSH 安装失败', detail: '安装命令失败', status: 'failed' },
              ],
              result: {},
              error_summary: '远程安装命令执行失败',
              updated_at: '2026-06-15T02:00:00Z',
            },
            {
              id: 'deploy-task-old-success',
              kind: 'agent_install',
              status: 'succeeded',
              title: 'Agent 一键安装',
              summary: 'Agent 安装成功',
              current_step: 'node_registered',
              progress_percent: 100,
              safe_metadata: {
                mode: 'one_click',
                access_node_name: '新加坡一键节点',
                public_host: 'agent.example.test',
                public_port: 443,
                install_dir: '/opt/xrayc/access-agent',
                compose_project: 'xrayc-access',
                ssh_port: 22,
                ssh_auth_method: 'password',
                force_reinstall: true,
              },
              steps: [
                { key: 'node_registered', title: '登记中转节点', detail: '已自动添加中转节点', status: 'done' },
              ],
              result: { access_node_id: 'node-one-click-1', auth_code_received: true },
              error_summary: '',
              updated_at: '2026-06-15T01:00:00Z',
            },
            {
              id: 'deploy_job_old_failed_covered_by_success',
              kind: 'agent_install',
              status: 'failed',
              title: 'Agent 一键安装',
              summary: '上传 Agent 安装脚本失败',
              current_step: 'ssh_install_failed',
              progress_percent: 100,
              safe_metadata: {
                mode: 'one_click',
                access_node_name: '已修复节点',
                public_host: 'covered.example.test',
                public_port: 443,
                install_dir: '/opt/xrayc/access-agent',
                compose_project: 'xrayc-access',
                ssh_port: 22,
                ssh_auth_method: 'password',
                force_reinstall: true,
              },
              steps: [
                { key: 'ssh_install_failed', title: 'SSH 安装失败', detail: '上传失败', status: 'failed' },
              ],
              result: {},
              error_summary: '上传 Agent 安装脚本失败',
              updated_at: '2026-06-15T01:30:00Z',
            },
            {
              id: 'deploy_job_new_success_covers_old_failure',
              kind: 'agent_install',
              status: 'succeeded',
              title: 'Agent 一键安装',
              summary: 'Agent 安装成功',
              current_step: 'node_registered',
              progress_percent: 100,
              safe_metadata: {
                mode: 'one_click',
                access_node_name: '已修复节点',
                public_host: 'covered.example.test',
                public_port: 443,
                install_dir: '/opt/xrayc/access-agent',
                compose_project: 'xrayc-access',
                ssh_port: 22,
                ssh_auth_method: 'password',
                force_reinstall: true,
              },
              steps: [
                { key: 'node_registered', title: '登记中转节点', detail: '已自动添加中转节点', status: 'done' },
              ],
              result: { access_node_id: 'node-covered-1', auth_code_received: true },
              error_summary: '',
              updated_at: '2026-06-15T02:30:00Z',
            },
          ],
        },
      }),
    });
  });

  await page.goto('/admin/transit-nodes');
  await expect(page.getByRole('button', { name: 'agent.example.test:443' })).toHaveCount(1);
  await expect(page.getByRole('button', { name: 'covered.example.test:443' })).toHaveCount(0);
  await expect(page.getByText('1 条历史')).toBeVisible();

  await page.getByRole('button', { name: 'agent.example.test:443' }).click();
  const detailDialog = page.getByRole('dialog', { name: '部署详情' });
  await expect(detailDialog).toBeVisible();
  await expect(detailDialog.getByText('错误摘要：远程安装命令执行失败')).toBeVisible();
  await expect(detailDialog.getByText('Agent 安装成功')).toHaveCount(0);
  await detailDialog.getByRole('button', { name: '重试安装' }).click();

  const installDialog = page.getByRole('dialog', { name: '一键安装 Agent' });
  await expect(installDialog).toBeVisible();
  await expect(installDialog.locator('.el-form-item').filter({ hasText: '节点名称' }).locator('input')).toHaveValue('新加坡一键节点');
  // 三地址自动推导后弹窗已无独立「公网地址」字段；重试也不回填任何直连地址（IP/域名/CF 需重填）。
  await expect(installDialog.locator('.el-form-item').filter({ hasText: '公网地址' })).toHaveCount(0);
  // SSH Host 是独立 SSH IP 字段，重试只回填任务原 ssh_host，绝不退回客户连接地址（public_host）。
  await expect(installDialog.locator('.el-form-item').filter({ hasText: 'SSH Host' }).locator('input')).toHaveValue('192.0.2.10');
  await expect(installDialog.getByText('一键安装会自动连接服务器、上传并执行脚本；无需手动上传。')).toBeVisible();
  await expect(installDialog.getByText('安装前会先卸载旧容器和旧运行状态。')).toBeVisible();
});
