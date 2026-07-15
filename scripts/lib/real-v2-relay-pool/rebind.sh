#!/usr/bin/env bash
# 用途：真实 relay-pool E2E 中按“入口-出口绑定节点”模型切换分组成员。

activate_line_group_binding_node() {
  local target_access_line_id="$1"
  local target_pool_id="$2"
  local reason="$3"
  local safe_reason="${reason//[^A-Za-z0-9_.-]/_}"
  local payload="$TMP_DIR/line-group-binding-nodes-${safe_reason}.json"
  local body="$TMP_DIR/line-group-binding-nodes-${safe_reason}-body.json"
  if [[ -n "$target_access_line_id" ]]; then
    ACCESS_LINE_ID_VALUE="$target_access_line_id" \
      write_json "$payload" 'print(json.dumps({"binding_node_ids": [os.environ["ACCESS_LINE_ID_VALUE"]]}, ensure_ascii=False))'
  else
    write_json "$payload" 'print(json.dumps({"binding_node_ids": []}, ensure_ascii=False))'
  fi
  api_json PUT "/api/admin/line-groups/${group_id}/binding-nodes" "$payload" "$body" "$admin_token"
  psql_db -Xq -v ON_ERROR_STOP=1 \
    -v access_line_id_a="${access_line_id_a:-00000000-0000-0000-0000-000000000000}" \
    -v access_line_id_b="${access_line_id_b:-00000000-0000-0000-0000-000000000000}" \
    -v pool_id_a="${pool_id_a:-00000000-0000-0000-0000-000000000000}" \
    -v pool_id_b="${pool_id_b:-00000000-0000-0000-0000-000000000000}" \
    -v access_node_id="$access_node_id" \
    -v reason="$reason" <<'SQL'
BEGIN;
DELETE FROM user_access_line_assignments
WHERE access_line_id IN (:'access_line_id_a'::uuid, :'access_line_id_b'::uuid);
DELETE FROM user_exit_assignments
WHERE access_line_id IN (:'access_line_id_a'::uuid, :'access_line_id_b'::uuid)
   OR exit_pool_id IN (:'pool_id_a'::uuid, :'pool_id_b'::uuid);
UPDATE access_nodes
SET config_dirty = TRUE,
    config_dirty_at = now(),
    desired_config_hash = NULL,
    config_dirty_reason = 'real_v2_binding_node_' || :'reason'
WHERE id = :'access_node_id'::uuid;
COMMIT;
SQL
  if [[ -n "$target_access_line_id" ]]; then
    access_line_id="$target_access_line_id"
    pool_id="$target_pool_id"
  fi
  echo "real_v2_relay_pool_e2e: line_group_binding_node_active reason=${reason}"
}
