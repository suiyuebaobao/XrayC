// 本文件是前端访问 XrayC 后端 API 的统一兼容入口。
// 实际请求已按认证、运营、中转路由和计费订阅拆到 api/clients 子模块。
// 页面仍可从 `@/services/api` 导入 apiClient 与业务类型，避免大规模改调用点。
// 新增 API 时应优先放入对应 clients/normalizers/types 文件，禁止把逻辑堆回这里。

import { authApi } from './api/clients/auth';
import { backupApi } from './api/clients/backup';
import { billingApi } from './api/clients/billing';
import { operationsApi } from './api/clients/operations';
import { paymentApi } from './api/clients/payment';
import { routingApi } from './api/clients/routing';

export type * from './api/types';
export { plannedApiPaths, rustApiPaths } from './api/paths';
export { request } from './api/http';

export const apiClient = {
  ...authApi,
  ...operationsApi,
  ...routingApi,
  ...billingApi,
  ...paymentApi,
  ...backupApi,
};
