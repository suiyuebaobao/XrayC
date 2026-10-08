# 中转节点多域名 + 全对象可编辑 设计

> 状态:已与用户确认大方向(简单版,非自动化轮换/非 SNI 多路复用/非泛域名)。本特性排在
> 「本机出口 SOCKS5/HTTP 真流量闭环」与多模式特性合并之后实现。

## 目标

1. 中转节点支持**多个直连域名 + 多个 CF 域名**(当前是单 `cert_domain` + 单 `cf_domain`)。
2. 入口、本机出口创建/编辑时可**下拉选用节点的哪个域名**。
3. 节点、入口、出口、本机出口**全部可编辑**;补上本机出口的就地编辑/删除(当前只能新建)。

## 非目标(用户已明确不要,避免过度设计)

- 不做自动探测封锁 + 自动切换(管理员用下拉手动切)。
- 不做 SNI 多路复用(一端口多证书)。每个入口仍只用它选中的那一个域名。
- 不做泛域名 `*.example.com` + per-user 子域名签发。
- 不做 per-user 直接分配域名的独立机制——"一用户一域名"靠给该用户的入口选指定域名实现。

## 使用场景(都靠"下拉选域名"实现,不需额外机制)

- **防封轮换**:节点囤多个已签好证书的备用域名;某域名被墙 → 把入口下拉切到另一个 → 订阅内容更新 → 客户端刷新订阅迁过去。目标域名证书早已就绪,切换即时。
- **多业务 / 一用户一域名**:不同入口在下拉里选不同域名。

## 数据模型

### node_domains(新表)

```
node_domains(
  id            uuid pk,
  access_node_id uuid fk -> access_nodes,
  domain        text not null,
  kind          text not null check (kind in ('direct','cf')),  -- 灰云直连 / 橙云 CF
  cf_cert_mode  text,            -- 仅 kind=cf:dns01 / reuse_direct
  acme_email    text,            -- 签证书用
  is_primary    boolean not null default false,  -- 该 kind 下的主域名(订阅/入口默认值)
  cert_status   text not null default 'unknown', -- unknown/valid/expiring/error(心跳回填)
  created_at    timestamptz not null default now(),
  unique (access_node_id, domain)
)
```

- 一节点多行;按 kind 分两组(direct 列表 + cf 列表)。
- `cf_enabled` 派生:节点存在任一 `kind='cf'` 的 node_domains 行即视为启用 CF。

### 迁移

- 把现有 `access_nodes.cert_domain` 搬一行进 `node_domains`(kind='direct', is_primary=true);
  `cf_domain` 搬一行(kind='cf', is_primary=true, cf_cert_mode 沿用)。
- `access_nodes.cert_domain`/`cf_domain`/`cf_cert_mode` 列保留为「主域名」快捷指针(读模型/订阅默认值
  仍可用),写入以 node_domains 为准;或后续清理为纯派生。迁移内保证两侧一致。

### 入口 / 本机出口引用域名

- `access_entries` 加 `node_domain_id uuid null`(免证书入口如 Reality/SS 留空)。
- `exit_endpoints`(本机出口 ownership=self_hosted)加 `node_domain_id uuid null`(免证书出口留空)。
- 删除 node_domains 行时,若仍被入口/出口引用则拒绝(先改引用再删),避免悬挂。

## 证书

- agent 对节点 `node_domains` 的**每个域名**各签一张证书:`kind='direct'` 走 HTTP-01,`kind='cf'` 走 DNS-01。
  现有单域名签发逻辑改成**遍历清单**,无新机制。
- CF 多域名各自在签发/续期时传 CF API Token(**不落库**,沿用 SSH 凭据红线);同一 CF 账号可复用一个
  token,不同账号各传各的。token 只在安装/续期请求期注入 agent env + 写节点本机 0600 ini。
- 注意 Let's Encrypt 限流:域名数量多时分批签、避免无谓重签(沿用现有 `x509 -checkend` 有效即保留)。

## 入口 / 出口选域名 + 护栏

- 创建/编辑入口、本机出口时:下拉列出该节点的 node_domains(按需按 kind 过滤)。
- **协议护栏按"选中域名的 kind"判定**(复用现有 `protocol_guardrails` + 出口侧 `validate_local_exit_cert_domain`,
  判定依据从「节点单域名」换成「选中的 node_domain」):
  - 选 CF 域名(kind=cf):入口只放 VLESS/Trojan + WS/gRPC/XHTTP + TLS;
  - 选直连域名(kind=direct):放 Trojan/HY2/VLESS-TLS;
  - 不选域名(留空):Reality / Shadowsocks(IP 直连免证书)。
- 订阅输出按入口选中的 node_domain 给出 SNI/连接地址;未选则用节点主域名/IP(行为同今天)。

## 全对象可编辑

| 对象 | 现状 | 本特性要做 |
|---|---|---|
| 中转节点 | 已可编辑(弹窗含域名字段) | 域名单字段 → 可增删的**列表**(direct 组 + cf 组),编辑弹窗内增删域名并触发对应签发/清理 |
| 入口 | 已可编辑 | 编辑弹窗加「选域名」下拉 |
| 出口(第三方/本机) | 已可编辑(出口管理页) | 保持;本机出口额外见下 |
| 本机出口服务 | **只能新建** | 补就地编辑/删除:本机出口页列出该节点已建 self_hosted 出口,直接改/删 |

- 本机出口就地编辑/删除:后端 `exit_endpoints` 的 `PUT`/`DELETE` 端点**已存在**,主要是前端把已建本机出口
  线路列出来 + 接 update/delete + 加「选域名」下拉;必要时补一个按节点查 self_hosted 出口的列表接口。

## 兼容性

- 单域名节点 = node_domains 里仅一行(is_primary),入口/出口默认用它,UX 与今天一致,零回归。
- 读模型(`model.rs`/`load.rs`/`read_models.rs`)补传 node_domains 列表 + 入口/出口选中的 node_domain;
  遵守「写侧有、读侧无」跨层对齐红线,真实 PG 测试核对。

## 测试要点(TDD,真实 PG + 真机闭环)

- node_domains 增删 + 迁移把旧单域名搬入 + 派生 cf_enabled。
- 入口/出口选 CF 域名 → 护栏只放 WS/gRPC/XHTTP+TLS;选直连域名 → 放 Trojan/HY2;留空 → Reality/SS。
- 删除被引用的 node_domain 被拒。
- agent 给多个域名各签证书(直连 HTTP-01 / CF DNS-01)——真机至少覆盖「2 个直连域名各 1 张证书」。
- 本机出口就地编辑/删除真 API + 前端 no-mock。
- 防封轮换闭环:同节点 2 个直连域名各 1 入口(或 1 入口切换域名),真实客户端经各域名出站、计费独立。

## 实现顺序(后续 writing-plans 细化)

1. 迁移 + node_domains 表 + DB store(增删查 + 迁移搬运)。
2. 证书签发遍历清单(agent + 部署脚本)。
3. 入口/出口加 node_domain_id + 护栏按选中域名判定 + 订阅输出。
4. 本机出口就地编辑/删除(接已有 exit_endpoints PUT/DELETE)。
5. 读模型对齐 + 前端(节点域名列表表单、入口/出口选域名下拉、本机出口编辑/删除)。
6. 真机闭环 + 防泄露 + 清理。
