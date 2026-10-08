# 单节点 SSL 状态上报与续期设计

## 目标

让每台中转节点的 `access-agent` 自动上报本机 SSL 证书状态，管理平台在“中转节点”卡片展示证书域名、到期时间、剩余天数和状态，并提供“续期 SSL 证书”按钮。续期动作按单个中转节点执行，不影响其他节点。

## 范围

- `access-agent` 从安装环境变量 `XRAYC_TLS_CERT_DOMAINS` 和当前已应用配置里的 TLS 证书路径合并出待检查域名。
- `access-agent` 用 `openssl x509 -noout -dates` 读取证书有效期，生成结构化状态并随心跳上报。
- 控制面把节点证书状态保存到 `access_nodes`，后台读模型返回给前端。
- 管理员点击单个节点的续期按钮后，控制面写入该节点的续期任务。
- `access-agent` 在下一次心跳收到续期任务，执行本机 `certbot renew --cert-name <domain> --non-interactive --no-random-sleep-on-renew`，成功后 reload Xray 并在后续心跳回报结果；默认不强制续期，避免触发 ACME 频率限制。
- 安装脚本申请证书时，只有域名和邮箱同时存在才自动申请；任一缺失则跳过自动申请并提示原因。

## 非目标

- 不做跨节点批量续期。
- 不把私钥路径、证书内容、真实服务器凭据或 certbot 原始日志回传到控制面。
- 不新增自动注册节点；仍然是安装脚本输出鉴权码，管理平台手动新增中转节点。
- 不改变 Caddy 作为管理平台 HTTPS 入口的架构。

## 数据模型

`access_nodes` 新增运行态字段：

- `tls_certificates jsonb not null default '[]'`：agent 上报的证书摘要数组。
- `tls_cert_last_report_at timestamptz`：最近证书上报时间。
- `tls_renew_request_id uuid`：最近续期请求 ID。
- `tls_renew_requested_at timestamptz`：最近续期请求时间。
- `tls_renew_completed_at timestamptz`：最近续期完成时间。
- `tls_renew_status text not null default ''`：`queued`、`running`、`success`、`failed` 或空。
- `tls_renew_message text not null default ''`：脱敏结果摘要。

证书摘要字段：

- `domain`
- `status`：`valid`、`expiring`、`expired`、`missing`、`invalid`
- `not_before`
- `not_after`
- `days_remaining`
- `error_summary`

## 接口

Agent 心跳请求新增：

- `tls_certificates`
- `tls_renew_result`

Agent 心跳响应新增：

- `tls_renew_task`：当该节点存在待执行请求时返回 `{ request_id, domains }`。

管理接口新增：

- `POST /api/admin/access-nodes/:access_node_id/tls/renew`

返回续期请求 ID、状态和域名列表。若节点没有可续期域名，返回业务错误。

## 前端

中转节点卡片展示证书状态：

- 没有证书上报：显示“未上报”。
- 有证书：展示第一个主域名、状态、到期时间和剩余天数；多个证书用一行摘要展示。
- 续期按钮放在每个节点卡片动作区，按钮 loading 只影响当前节点。

## 错误处理

- `certbot` 不存在时，agent 上报失败摘要，不自动安装包。
- 单个域名续期失败不阻塞其他域名，最终状态只要有失败就是 `failed`。
- 续期过程中不回传原始 stderr，只回传脱敏摘要。
- 续期成功后调用既有 `XRAYC_XRAY_RELOAD_COMMAND`，reload 失败记录为失败，避免证书更新但运行态未生效。

## 测试

- Rust 单测覆盖 agent 心跳 JSON 增加证书字段和续期任务解析。
- Rust 单测覆盖证书收集函数对有效、缺失、无效证书的分类。
- PostgreSQL 集成测试覆盖续期请求入队、心跳下发任务、续期结果落库。
- 前端构建验证类型、normalizer 和卡片按钮。
- 脚本静态测试覆盖域名和邮箱都存在才自动申请证书。
