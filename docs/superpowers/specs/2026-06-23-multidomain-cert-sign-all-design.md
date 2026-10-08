# 多域名证书:agent 自己签发全部域名 设计(concern #57)

> 已与用户确认:模型是"配几个域名就签几个;编辑换/加域名就重新签那个",**全自动、不用再 SSH**。
> 因此 agent 必须自己能签 —— 这要求 agent 容器内有 certbot(当前缺失,是唯一根因)。

## 问题

真机测试(no-mock)发现:节点配 2 个直连域名,但只签出主域名证书;且**编辑节点换/加域名后无法自动重签**。
根因:Track B 给 agent 写好了"遍历 node_domains 逐个签"的逻辑,但 **access-agent 容器镜像内没装 certbot**(`Command::new(certbot)` 在容器内找不到),签发任务一直失败重试。

## 方案 A:agent 容器内置 certbot,自驱签发/重签/续期

让 Track B 已有的 agent 签发逻辑真正能跑。覆盖三个时机,全程无需再 SSH:
1. **装机**:agent 从心跳 `node_domains` 遍历,逐个域名签(已有 `x509 -checkend` 跳过有效的)。
2. **编辑换/加域名**:控制面改 node_domains 后置节点 dirty / 下发 renew 任务 → agent 对清单逐个核验,缺的/换的新域名当场签(只签新的)。
3. **续期**:到期前 agent renew。

### 改动

1. **agent 镜像**(`./Dockerfile`,base `rust:1-bookworm`):`apt-get install -y certbot`(直连 HTTP-01 用)+ `python3-certbot-dns-cloudflare`(CF 域名 DNS-01 用)。
2. **签发 challenge 选择**(已有 `build_cert_plan`):
   - `kind=direct`:优先 **DNS-01**(若该域名 zone 在 CF 且有 CF token,免占端口最稳);否则 **HTTP-01 standalone**(需容器可用 80 端口)。
   - `kind=cf`:DNS-01(CF token)。
   - MVP:直连域名走 HTTP-01 standalone(与现宿主脚本一致),agent 容器需能绑 80 端口签发瞬间用;CF 域名走 DNS-01。
3. **容器运行条件**:
   - `/etc/letsencrypt` 挂成**可写卷**(agent 写证书,xray 读)——确认 agent compose 当前挂载方式,改可写。
   - HTTP-01 需要**签发瞬间占 80 端口**:确认 agent 容器网络/端口,签发时临时用 80(签完释放);若 80 被占,回退该域名 DNS-01。
   - CF token 已由安装期注入 agent env(`XRAYC_CLOUDFLARE_API_TOKEN`),写容器内 0600 ini。
4. **触发**:编辑节点 domains(PUT)后,控制面对该节点下发 tls 签发/renew 任务(复用现有 `POST /tls/renew` 链路);agent 收到后跑 `build_cert_plan` 对全部 node_domains 逐个核验+签缺的。证书签出后 reload xray。

## 非目标

- 不改宿主部署脚本的安装期签发(它仍可作首次签的兜底);本设计让 agent 成为**持续**签发/续期的主体,使"编辑即重签"无需 SSH。
- 多 CF 域名同账号一个 token 即可;不同账号多 token 记为后续。

## 测试

- agent 单测:`build_cert_plan` 对 [d1 direct, d2 direct, cf] → HTTP-01×2 + DNS-01×1(已有);新增"容器内 certbot 存在性"在镜像构建后 smoke(`certbot --version`)。
- 真机(节点A 2 直连域名):
  - 一键安装 → agent 自动签出**两张独立证书**(不再需 2-SAN 绕行),各 `/etc/letsencrypt/live/{域名}/`。
  - **编辑节点加第 3 个直连域名**(relay-c) → 无需 SSH,agent 自动签出第 3 张证书;入口切到 relay-c 真实客户端连通。
  - 证书续期路径:模拟 `--force-renewal` 或 checkend 触发,agent 重签成功 reload xray。
- 防泄露:CF token 不入日志/审计/心跳/仓库。
