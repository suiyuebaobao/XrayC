// 本文件集中提供中转节点页面各弹窗表单的默认值。
// useAccessLinesPage 通过这些工厂重置表单，组件只负责绑定展示。
// VLESS 默认使用 TCP；Reality 固定 TCP，普通 VLESS 可按需追加 UDP/XUDP。
// 文件不请求接口、不读写页面状态，只返回新的表单草稿对象。
import {
  defaultLocalExitLineDetails,
  normalizeLocalExitCarriage,
  normalizeLocalExitTransports,
} from '@/views/access-lines/localExitLines';
import type {
  InstallGuideForm,
  LocalExitLineForm,
  LocalExitLinesForm,
  NodeForm,
  OneClickInstallForm,
} from '@/views/access-lines/types';

export function emptyNodeForm(): NodeForm {
  return {
    name: '',
    publicPort: 443,
    sshHost: '',
    agentToken: '',
    remark: '',
    ipDirectAddress: '',
    domains: [],
    acmeEmail: '',
  };
}

export function emptyLocalExitLinesForm(nodeId = '', host = ''): LocalExitLinesForm {
  return {
    nodeId,
    regionCode: '',
    // 默认只预填免证书协议(VLESS-Reality + Shadowsocks):预填行默认 IP 直连模式都适用。
    // HY2/Trojan 要域名直连地址 + 证书,纯 IP 节点用不了,不预填;需要时管理员把某行
    // 「连接方式」切到「域名直连/CF 直连」再选这些协议(节点有对应域名时才出现)。
    lines: [
      emptyLocalExitLineForm('vless', host, 1443),
      emptyLocalExitLineForm('shadowsocks', host, 1444),
    ],
  };
}

export function emptyLocalExitLineForm(
  outboundType: LocalExitLineForm['outboundType'] = 'vless',
  host = '',
  port = 1443,
): LocalExitLineForm {
  const details = defaultLocalExitLineDetails(outboundType, host);
  return {
    resourceName: '',
    endpointName: '',
    outboundType,
    // 传输默认取协议第一项（SS/SOCKS/HTTP/VLESS-Reality 为 RAW，HY2 为 udp）；承载默认取协议第一个 L4 项。
    transports: normalizeLocalExitTransports(outboundType, details.vlessSecurity, []),
    carriage: normalizeLocalExitCarriage(outboundType, details.vlessSecurity, ''),
    ...details,
    host,
    port,
    nodeDomainId: '',
    enabled: true,
  };
}

export function emptyInstallGuideForm(): InstallGuideForm {
  return {
    nodeName: '',
    publicHost: '',
    installDir: '/opt/xrayc/access-agent',
    composeProjectName: 'xrayc-access',
    panelUrl: '',
    xrayApiPort: 10085,
    tlsCertDomains: '',
    forceReinstall: true,
    disableLegacySystemdUnits: true,
  };
}

export function emptyOneClickInstallForm(): OneClickInstallForm {
  return {
    nodeName: '',
    publicPort: 443,
    remark: '',
    sshHost: '',
    sshPort: 22,
    sshUser: 'root',
    sshAuthMethod: 'password',
    sshPassword: '',
    sshPrivateKey: '',
    installDir: '/opt/xrayc/access-agent',
    composeProject: 'xrayc-access',
    ipDirectAddress: '',
    certDomain: '',
    cfDomain: '',
    acmeEmail: '',
    cfApiToken: '',
    forceReinstall: true,
  };
}
