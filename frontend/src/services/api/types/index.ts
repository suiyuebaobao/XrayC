// 本文件统一导出前端 API 类型。
// `frontend/src/services/api.ts` 通过这里保留原有类型导出兼容性。
// 新页面可以继续从 `@/services/api` 导入类型，后续也可逐步改为领域路径。
// 此文件不定义业务结构，只作为类型 barrel，避免单文件继续膨胀。

export type * from './auth';
export type * from './backup';
export type * from './billing';
export type * from './nodeDomains';
export type * from './operations';
export type * from './payment';
export type * from './routing';
export type * from './sales';
