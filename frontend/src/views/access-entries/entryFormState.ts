// 入口表单状态类型定义。
// 职责：集中声明 AccessEntryDialog / AccessEntriesPage 共用的入口表单字段结构，
//   从 Dialog 组件抽出便于复用并守住单文件行数上限。仅类型声明，无运行逻辑。
import type { EntryAddressType } from '@/views/access-entries/entryProtocolMatrix';

export interface AccessEntryFormState {
  id: string;
  accessNodeId: string;
  name: string;
  listenHost: string;
  listenPort: number;
  protocol: string;
  transport: string;
  // 传输（streamSettings.network，可多选，每选一个生成一条入口）：RAW(tcp)/XHTTP/WS/gRPC，HY2 固定 hysteria。
  transports: string[];
  // 承载（L4 单选合并）：tcp / tcp,udp / tcp,udp,xudp；提交时降维成入口接受的单值模式。
  carriage: string;
  security: string;
  serverName: string;
  // VLESS 量子加密（后量子）开关：仅非 Reality(TLS/普通 none/CF) 有效，提交转 snake vless_quantum_encryption。
  vlessQuantumEncryption: boolean;
  wsPath: string;
  wsHost: string;
  cdnEnabled: boolean;
  cdnProvider: string;
  cdnHostname: string;
  // 选中的节点域名 id（免证书入口留空）；护栏按选中域名 kind 过滤。
  nodeDomainId: string;
  // 连接方式（地址类型）：ip=IP 直连免证书；domain=域名直连灰云；cf=CF 直连橙云。显式驱动护栏。
  connectionMode: EntryAddressType;
  enabled: boolean;
  sortWeight: number;
}
