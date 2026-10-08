// 版本响应只接收明确回报的字段；缺少 API 版本时视为读取失败，不拼造运行版本。
import { arrayValue, recordValue, stringValue } from './primitives';

export function normalizeSystemInfo(value: unknown) {
  const data = recordValue(value);
  const api = recordValue(data.api);
  if (!stringValue(api.release_id)) throw new Error('服务尚未提供版本信息');
  return {
    api: { environment: stringValue(api.environment) || '未标明', releaseId: stringValue(api.release_id), packageVersion: stringValue(api.package_version),
      platform: `${stringValue(api.target_os)}/${stringValue(api.target_arch)}` },
    workers: arrayValue(data.workers).map((value) => {
      const row = recordValue(value);
      return { instanceId: stringValue(row.instance_id), releaseId: stringValue(row.release_id),
        packageVersion: stringValue(row.package_version), heartbeatAt: stringValue(row.heartbeat_at), fresh: row.fresh === true };
    }),
    nodes: arrayValue(data.nodes).map((value) => {
      const row = recordValue(value);
      return { id: stringValue(row.id), name: stringValue(row.name), agentVersion: stringValue(row.agent_version),
        xrayVersion: stringValue(row.xray_version), lastHeartbeatAt: stringValue(row.last_heartbeat_at) };
    }),
  };
}
export type SystemInfo = ReturnType<typeof normalizeSystemInfo>;
