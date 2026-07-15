// 本文件提供前端 API 响应标准化的基础工具函数。
// 它只处理 unknown 到字符串、数字、数组、对象和流量单位的安全转换。
// 这些函数不访问网络、不读写 session，也不包含具体业务 normalizer。
// 拆出本文件是为了减少 services/api.ts 巨型文件体积。

export function collectionValue(value: unknown, keys: string[]): unknown[] {
  if (Array.isArray(value)) {
    return value;
  }

  const data = recordValue(value);
  for (const key of keys) {
    const candidate = data[key];
    if (Array.isArray(candidate)) {
      return candidate;
    }
  }

  return [];
}

export function stringArrayValue(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value.map(stringValue).filter(Boolean);
  }

  if (typeof value === 'string') {
    return value
      .split('\n')
      .map((item) => item.trim())
      .filter(Boolean);
  }

  return [];
}

export function arrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

export function recordValue(value: unknown): Record<string, unknown> {
  return isRecord(value) ? value : {};
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function stringValue(value: unknown) {
  return typeof value === 'string' ? value : '';
}

export function textValue(value: unknown) {
  if (typeof value === 'string') {
    return value;
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return '';
}

export function booleanValue(value: unknown) {
  return value === true || value === 'true' || value === 1 || value === '1';
}

export function numberValue(value: unknown) {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : 0;
  }

  if (typeof value === 'string' && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : 0;
  }

  return 0;
}

export function idValue(value: unknown) {
  if (typeof value === 'string' && value) {
    return value;
  }

  return numberValue(value);
}

export function createdId(data: Record<string, unknown>) {
  return stringValue(data.id);
}

export function clampNumber(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

export function nullableNumberValue(value: unknown) {
  if (value === null || value === undefined || value === '') {
    return null;
  }

  return numberValue(value);
}

export function bytesToGb(bytes: number) {
  return bytes / 1024 ** 3;
}

export function nullableBytesToGb(value: unknown) {
  const bytes = nullableNumberValue(value);
  return bytes === null ? null : bytesToGb(bytes);
}

export function gbToBytes(gb: number) {
  return Math.round(Math.max(0, gb) * 1024 ** 3);
}

export function optionalNumberValue(value: unknown) {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }

  if (typeof value === 'string' && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }

  return null;
}

export function appendQueryParam(params: URLSearchParams, key: string, value: string | number | null | undefined) {
  if (value === null || value === undefined || value === '') {
    return;
  }

  params.set(key, String(value));
}
