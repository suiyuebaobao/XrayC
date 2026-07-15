// 本文件集中 Reality「伪装域名」(serverName/dest) 的分类下拉数据，供入口与本机出口共用。
// 两组：支持后量子(TLS 握手协商 X25519MLKEM768) / 普通伪装(仅 X25519)，列表为实测可用大站。
// 用户可在 el-select(allow-create) 手输列表外域名；默认 www.cloudflare.com。
// 文件只提供静态选项与默认值，不持有状态、不请求接口。

// Reality 伪装域名默认值（免证书 decoy，与后端 server_names 兜底口径一致）。
export const DEFAULT_DECOY_DOMAIN = 'www.cloudflare.com';

export type DecoyDomainGroup = {
  label: string;
  options: Array<{ label: string; value: string }>;
};

// 【支持后量子 · X25519MLKEM768】：实测带 SNI 握手能协商后量子密钥交换的伪装目标。
const QUANTUM_DECOY_DOMAINS = [
  'www.apple.com',
  'swcdn.apple.com',
  'swdist.apple.com',
  'gateway.icloud.com',
  'www.cloudflare.com',
  'www.amazon.com',
  'aws.amazon.com',
  'dl.google.com',
  'www.google.com',
  'www.amd.com',
  'www.python.org',
  'addons.mozilla.org',
  'www.wikipedia.org',
  'www.yahoo.com',
  'www.paypal.com',
  'www.visa.com',
  'www.qualcomm.com',
  'www.ibm.com',
];

// 【普通伪装 · 仅 X25519】：可用作 Reality dest，但 TLS 握手不协商后量子。
const PLAIN_DECOY_DOMAINS = [
  'www.microsoft.com',
  'www.bing.com',
  'www.nvidia.com',
  'www.samsung.com',
  'www.tesla.com',
  'www.intel.com',
  'www.adobe.com',
  'www.salesforce.com',
  'www.oracle.com',
];

const toOptions = (domains: string[]) => domains.map((domain) => ({ label: domain, value: domain }));

// 伪装域名分组选项（el-option-group 直接消费）。
export const decoyDomainGroups: DecoyDomainGroup[] = [
  { label: '支持后量子（X25519MLKEM768）', options: toOptions(QUANTUM_DECOY_DOMAINS) },
  { label: '普通伪装（仅 X25519）', options: toOptions(PLAIN_DECOY_DOMAINS) },
];
