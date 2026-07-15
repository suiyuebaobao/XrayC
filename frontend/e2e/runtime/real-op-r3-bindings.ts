/*
 * 用途：并行流3·R3 真实 UI 绑定步骤的只读核验辅助（从 real-op-phaseR3.spec.ts 抽出守 550 行）。
 * 这里只封装「在浏览器上下文读 admin 接口、核验绑定是否真落库」的逻辑，不修改业务实现、不做 SQL 直插。
 * 抽出原因：上轮 phaseC 绑定 0 落库的根因是 UI 选项未真正选上、保存被前端护栏静默拦下，
 * 故每步绑定后必须只读核验 access_entry_exit_bindings / line_group_binding_nodes 真非空。
 */
import type { Page } from '@playwright/test';

/** 浏览器上下文里取当前管理员 access token（与 spec 内联读法一致）。 */
const TOK = "const sess=JSON.parse(localStorage.getItem('xrayc.session')||'{}');const tok=sess.accessToken||sess.access_token||'';";

/** 已存在出口绑定的入口名集合（命名空间内）。create-binding 是追加语义，重绑会产生重复，故先查既有。 */
export async function boundEntryNames(page: Page, ns: string): Promise<string[]> {
  return page.evaluate(async (nsArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const er = await fetch('/api/admin/access-entries', { headers: { Authorization: `Bearer ${tok}` } });
    const ej = er.ok ? await er.json() : null;
    const entries = ej?.data?.accessEntries ?? ej?.data?.access_entries ?? ej?.data ?? [];
    const br = await fetch('/api/admin/access-entry-exit-bindings', { headers: { Authorization: `Bearer ${tok}` } });
    const bj = br.ok ? await br.json() : null;
    const binds = bj?.data?.access_entry_exit_bindings ?? bj?.data ?? [];
    const boundIds = new Set((Array.isArray(binds) ? binds : []).map((b: any) => String(b.access_entry_id ?? b.accessEntryId)));
    return (Array.isArray(entries) ? entries : [])
      .filter((e: any) => String(e.name).startsWith(nsArg) && boundIds.has(String(e.id)))
      .map((e: any) => String(e.name));
  }, ns).catch(() => [] as string[]);
}

/** 只读核验：指定入口名是否真有出口绑定落库（access_entry_exit_bindings）。 */
export async function entryHasBinding(page: Page, entryName: string): Promise<boolean> {
  return page.evaluate(async (nameArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const er = await fetch('/api/admin/access-entries', { headers: { Authorization: `Bearer ${tok}` } });
    const ej = er.ok ? await er.json() : null;
    const entries = ej?.data?.accessEntries ?? ej?.data?.access_entries ?? ej?.data ?? [];
    const entry = (Array.isArray(entries) ? entries : []).find((e: any) => String(e.name) === nameArg);
    if (!entry?.id) return false;
    const br = await fetch('/api/admin/access-entry-exit-bindings', { headers: { Authorization: `Bearer ${tok}` } });
    const bj = br.ok ? await br.json() : null;
    const binds = bj?.data?.access_entry_exit_bindings ?? bj?.data ?? [];
    return (Array.isArray(binds) ? binds : []).some((b: any) => String(b.access_entry_id ?? b.accessEntryId) === String(entry.id));
  }, entryName).catch(() => false);
}

/** 只读核验：命名空间内有多少条入口已落出口绑定（去重 entry id）。 */
export async function nsBoundEntryCount(page: Page, ns: string): Promise<number> {
  return page.evaluate(async (nsArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const er = await fetch('/api/admin/access-entries', { headers: { Authorization: `Bearer ${tok}` } });
    const ej = er.ok ? await er.json() : null;
    const entries = ej?.data?.accessEntries ?? ej?.data?.access_entries ?? ej?.data ?? [];
    const nsIds = new Set((Array.isArray(entries) ? entries : [])
      .filter((e: any) => String(e.name).startsWith(nsArg)).map((e: any) => String(e.id)));
    const br = await fetch('/api/admin/access-entry-exit-bindings', { headers: { Authorization: `Bearer ${tok}` } });
    const bj = br.ok ? await br.json() : null;
    const binds = bj?.data?.access_entry_exit_bindings ?? bj?.data ?? [];
    const boundIds = new Set((Array.isArray(binds) ? binds : [])
      .map((b: any) => String(b.access_entry_id ?? b.accessEntryId)).filter((id: string) => nsIds.has(id)));
    return boundIds.size;
  }, ns).catch(() => 0);
}

// 注意：分组读模型在 /api/admin/access-routing 的 line_groups（带 binding_node_ids），
// 不存在 GET /api/admin/line-groups（那是 405，只有 POST/PUT/DELETE）。上轮 step8 伪失败根因
// 就是误读 405 端点把真有 5 个绑定节点的分组读成 0。

/** 只读核验：分组的绑定节点条数（line_group_binding_nodes / binding_node_ids 读模型）。 */
export async function groupBindingNodeCount(page: Page, groupId: string): Promise<number> {
  return page.evaluate(async (gid) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch('/api/admin/access-routing', { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    const items = j?.data?.line_groups ?? j?.data?.lineGroups ?? [];
    const g = (Array.isArray(items) ? items : []).find((x: any) => String(x.id) === String(gid));
    const ids = g?.binding_node_ids ?? g?.bindingNodeIds ?? g?.binding_nodes ?? g?.bindingNodes ?? [];
    return Array.isArray(ids) ? ids.length : 0;
  }, groupId).catch(() => 0);
}

/** 幂等：按名查既有分组 id（读 access-routing 的 line_groups）。 */
export async function findGroupId(page: Page, name: string): Promise<string> {
  return page.evaluate(async (nameArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch('/api/admin/access-routing', { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    const items = j?.data?.line_groups ?? j?.data?.lineGroups ?? [];
    const f = Array.isArray(items) ? items.find((g: any) => g.name === nameArg) : null;
    return f?.id ? String(f.id) : '';
  }, name).catch(() => '');
}

/** 幂等：按名查既有套餐 id。 */
export async function findPlanId(page: Page, name: string): Promise<string> {
  return page.evaluate(async (nameArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch('/api/admin/plans', { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    const items = j?.data?.plans ?? j?.data?.items ?? j?.data ?? [];
    const f = Array.isArray(items) ? items.find((p: any) => p.name === nameArg) : null;
    return f?.id ? String(f.id) : '';
  }, name).catch(() => '');
}

/** 幂等：按 email 查既有用户 id。 */
export async function findUserId(page: Page, email: string): Promise<string> {
  return page.evaluate(async (emailArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch('/api/admin/users?search=' + encodeURIComponent(emailArg), { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    const items = j?.data?.users ?? j?.data?.items ?? j?.data ?? [];
    const f = Array.isArray(items) ? items.find((u: any) => u.email === emailArg) : null;
    return f?.id ? String(f.id) : '';
  }, email).catch(() => '');
}

/** 幂等：本命名空间名下的出口端点数量（步骤5 非幂等，已有就跳过避免重复堆叠拖垮 agent reconcile）。 */
export async function nsExitEndpointCount(page: Page, ns: string): Promise<number> {
  return page.evaluate(async (nsArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch('/api/admin/exit-endpoints', { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    const eps = j?.data?.exitEndpoints ?? j?.data?.exit_endpoints ?? j?.data?.items ?? j?.data ?? [];
    return (Array.isArray(eps) ? eps : []).filter((e: any) => String(e.name ?? '').startsWith(nsArg)).length;
  }, ns).catch(() => 0);
}

/** 读节点健康：返回 {id, health, hb} 供心跳等待用。 */
export async function fetchNodeHealth(page: Page, name: string): Promise<{ id: string; health: string; hb: unknown }> {
  return page.evaluate(async (nameArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch('/api/admin/access-nodes', { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    const nodes = j?.data?.accessNodes ?? j?.data?.access_nodes ?? j?.data ?? [];
    const f = Array.isArray(nodes) ? nodes.find((n: any) => n.name === nameArg) : null;
    return {
      id: f?.id ? String(f.id) : '',
      health: String(f?.healthStatus ?? f?.health_status ?? ''),
      hb: f?.lastHeartbeatAt ?? f?.last_heartbeat_at ?? null,
    };
  }, name).catch(() => ({ id: '', health: '', hb: null }));
}

/** 幂等：按名查既有入口 id（入口创建非幂等，重跑前先查，存在即跳过创建）。 */
export async function findEntryId(page: Page, name: string): Promise<string> {
  return page.evaluate(async (nameArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch('/api/admin/access-entries', { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    const es = j?.data?.accessEntries ?? j?.data?.access_entries ?? j?.data ?? [];
    const f = Array.isArray(es) ? es.find((e: any) => e.name === nameArg) : null;
    return f?.id ? String(f.id) : '';
  }, name).catch(() => '');
}

/** 幂等：按名查既有中转节点 id。 */
export async function findNodeId(page: Page, name: string): Promise<string> {
  return page.evaluate(async (nameArg) => {
    const sess = JSON.parse(localStorage.getItem('xrayc.session') || '{}');
    const tok = sess.accessToken || sess.access_token || '';
    const r = await fetch('/api/admin/access-nodes', { headers: { Authorization: `Bearer ${tok}` } });
    const j = r.ok ? await r.json() : null;
    const nodes = j?.data?.accessNodes ?? j?.data?.access_nodes ?? j?.data ?? [];
    const f = Array.isArray(nodes) ? nodes.find((n: any) => n.name === nameArg) : null;
    return f?.id ? String(f.id) : '';
  }, name).catch(() => '');
}
void TOK;
