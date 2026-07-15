// 中文说明：备份状态 normalizer 单测，聚焦新增的 history 历史记录解析
// （snake→camel、缺省/非数组兜底、字节非负、空字段折 null、异地三态），node 环境无 DOM。
import { describe, expect, it } from 'vitest';
import { normalizeBackupState } from './backup';

describe('normalizeBackupState - history 历史记录解析', () => {
  it('snake_case 逐条解析并保持后端顺序（最新在前）', () => {
    const state = normalizeBackupState({
      full: {
        history: [
          { at: '2026-07-01T03:00:00Z', ok: true, size_bytes: 2048, file: 'a.dump', offsite_synced: true, error: null },
          { at: '2026-06-30T03:00:00Z', ok: false, size_bytes: 0, file: null, offsite_synced: false, error: '连接超时' },
        ],
      },
      wal: {},
      email: {},
    });
    expect(state.full.history).toEqual([
      { at: '2026-07-01T03:00:00Z', ok: true, sizeBytes: 2048, file: 'a.dump', offsiteSynced: true, error: null },
      { at: '2026-06-30T03:00:00Z', ok: false, sizeBytes: 0, file: null, offsiteSynced: false, error: '连接超时' },
    ]);
  });

  it('camelCase 亦可解析（sizeBytes/offsiteSynced）', () => {
    const state = normalizeBackupState({
      full: { history: [{ at: 't', ok: true, sizeBytes: 100, file: 'x', offsiteSynced: false, error: 'e' }] },
      wal: {},
      email: {},
    });
    expect(state.full.history[0]).toEqual({
      at: 't',
      ok: true,
      sizeBytes: 100,
      file: 'x',
      offsiteSynced: false,
      error: 'e',
    });
  });

  it('缺失或非数组的 history 一律回退空数组', () => {
    const state = normalizeBackupState({ full: {}, wal: { history: 'nope' }, email: { history: {} } });
    expect(state.full.history).toEqual([]);
    expect(state.wal.history).toEqual([]);
    expect(state.email.history).toEqual([]);
  });

  it('空文件名/错误折成 null，负数/非数字字节归零', () => {
    const state = normalizeBackupState({
      full: { history: [{ at: 't', ok: true, size_bytes: -5, file: '', error: '' }] },
      wal: {},
      email: {},
    });
    expect(state.full.history[0]).toMatchObject({ sizeBytes: 0, file: null, error: null });
  });

  it('异地保持三态：缺失或非布尔归 null', () => {
    const state = normalizeBackupState({
      full: { history: [{ at: 't', ok: true, size_bytes: 1 }] },
      wal: {},
      email: {},
    });
    expect(state.full.history[0].offsiteSynced).toBeNull();
  });
});
