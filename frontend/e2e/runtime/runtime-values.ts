/*
 * 用途：提供 runtime no-mock e2e 的通用数据读取工具。
 * 这些函数只处理测试中的未知后端响应形状。
 */
export type JsonRecord = Record<string, unknown>;

export function collection(value: unknown, keys: string[]): unknown[] {
  if (Array.isArray(value)) {
    return value;
  }
  const data = recordValue(value);
  for (const key of keys) {
    if (Array.isArray(data[key])) {
      return data[key];
    }
  }
  return [];
}

export function recordValue(value: unknown): JsonRecord {
  return isRecord(value) ? value : {};
}

export function isRecord(value: unknown): value is JsonRecord {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

export function stringValue(value: unknown) {
  if (typeof value === 'string') {
    return value;
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return '';
}

export function numberValue(value: unknown) {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : 0;
  }
  return 0;
}

export function optionalNumberValue(value: unknown): number | null {
  if (value === null || value === undefined || value === '') {
    return null;
  }
  return numberValue(value);
}

export function nullableNumberValue(value: unknown): number | null {
  if (value === null || value === undefined || value === '') {
    return null;
  }
  return numberValue(value);
}
