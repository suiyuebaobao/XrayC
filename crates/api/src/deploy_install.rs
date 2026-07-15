//! Agent 安装执行辅助模块。
//! 本文件承接一键安装请求解析、安装环境变量生成和后台 SSH 任务。
//! 这里不注册 HTTP 路由，只服务 deploy handler 和相关测试。
//! SSH 密码、私钥、部署 token 只在请求生命周期内流转。
//! 安装输出只保存脱敏摘要，鉴权码解析后立即进入节点登记流程。
//! 手动安装和一键安装共用环境变量生成逻辑，避免脚本契约分叉。
//! SSL 域名与邮箱校验沿用部署校验模块，不在这里放宽规则。
//! 后台任务通过 deployment_tasks 表持续回写进度。
//! 文件前十行中文注释满足仓库规则。
//! 请勿在源码注释或日志中写入服务器敏感信息。

use super::*;

// 安装步骤 JSON 构造已拆到 deploy_install_steps;重导出使 deploy.rs 的 glob import 不变。
pub(crate) use crate::deploy_install_steps::{
    install_task_steps_json, one_click_install_task_steps_json,
};

pub(crate) struct OneClickAgentInstallResolved {
    pub(crate) install: AgentInstallGuideResolved,
    pub(crate) name: String,
    pub(crate) public_host: String,
    pub(crate) public_port: u16,
    pub(crate) remark: String,
    pub(crate) cert_domain: Option<String>,
    pub(crate) acme_email: String,
    pub(crate) cf_enabled: bool,
    pub(crate) cf_domain: Option<String>,
    pub(crate) ip_direct_address: Option<String>,
    pub(crate) ssh_host: String,
    pub(crate) ssh_port: u16,
    pub(crate) ssh_user: String,
    pub(crate) ssh_password: Option<String>,
    pub(crate) ssh_private_key: Option<String>,
}

pub(crate) async fn resolve_one_click_agent_install_request(
    headers: &HeaderMap,
    body: OneClickAgentInstallRequest,
) -> Result<OneClickAgentInstallResolved, String> {
    let name = validate_request_text(body.name, "name", 128)?;
    if name.is_empty() {
        return Err("中转节点名称不能为空".to_string());
    }
    let public_host = validate_request_text(body.public_host, "public_host", 255)?;
    if public_host.is_empty() {
        return Err("中转节点地址不能为空".to_string());
    }
    let public_port = require_nonzero_port(body.public_port.unwrap_or(443), "public_port")?;
    let remark = optional_request_text(body.remark, "remark", 512)?.unwrap_or_default();
    let ssh_host = validate_request_text(body.ssh_host, "ssh_host", 255)?;
    if ssh_host.is_empty() {
        return Err("SSH 主机不能为空".to_string());
    }
    // 一键安装会在本次请求期内直连 SSH 部署;SSH 目标必须是 IP。
    // 域名(尤其橙云/CF 代理)无法直连 SSH,这里直接拒绝,避免后续连接必然失败。
    // public_host(客户端连接地址)不受此限,仍可为 IP 或域名。
    if ssh_host.parse::<std::net::IpAddr>().is_err() {
        return Err(
            "一键安装的 SSH 地址必须是 IP,不能是域名(域名尤其橙云无法直连 SSH)".to_string(),
        );
    }
    let ssh_port = require_nonzero_port(body.ssh_port.unwrap_or(22), "ssh_port")?;
    let ssh_user = validate_request_text(body.ssh_user, "ssh_user", 128)?;
    if ssh_user.is_empty() {
        return Err("SSH 用户不能为空".to_string());
    }
    let ssh_password = optional_request_text(body.ssh_password, "ssh_password", 4096)?;
    let ssh_private_key =
        optional_multiline_secret_text(body.ssh_private_key, "ssh_private_key", 20_000)?;
    if ssh_password.is_none() && ssh_private_key.is_none() {
        return Err("SSH 密码或私钥必须填写一个".to_string());
    }
    if ssh_password.is_some() && ssh_private_key.is_some() {
        return Err("SSH 密码和私钥只能选择一种".to_string());
    }

    // 控制面地址优先级(CLAUDE.md §13):后端注入的稳定公网中心地址 XRAYC_PUBLIC_BASE_URL 最高,
    // 其次才是请求体传入值(前端浏览器 origin 可能是本机/IP/备用域名,远端 agent 回拉制品/回连会不可达),
    // 最后回退请求头推断。只有这三者都为空才报错。这样面板从非公网地址访问时仍下发可达的中心地址。
    let control_plane_url = env_nonempty("XRAYC_PUBLIC_BASE_URL")
        .or(optional_request_text(
            body.control_plane_url,
            "control_plane_url",
            2048,
        )?)
        .or_else(|| infer_control_plane_url(headers))
        .ok_or_else(|| "control_plane_url 不能为空".to_string())?;
    let deploy_artifact_token = env_nonempty("DEPLOY_ARTIFACT_TOKEN")
        .ok_or_else(|| "DEPLOY_ARTIFACT_TOKEN 未配置，无法一键安装".to_string())?;
    let mut tls_cert_domains = normalize_tls_cert_domains(body.tls_cert_domains, Vec::new())?;
    let acme_email = validate_acme_email(body.acme_email)?;
    if tls_cert_domains.is_empty() && !acme_email.is_empty() && looks_like_tls_domain(&public_host)
    {
        tls_cert_domains.push(public_host.clone());
    }
    // 提取节点级 CF 字段:启用 CF 且配了直连证书域名时把 cert_domain 并入证书申请清单(certbot
    // 安装阶段签出它,CF 回源复用)。ip_direct_address 透传进节点创建,纯 IP 节点装完无需再单独 PUT 补。
    // 域名直连地址在一键安装里是经 tls_cert_domains 传来(供 agent 签 HTTP-01 证书)的;
    // 调用方(含当前前端)若没单独给 cert_domain,取首个待签直连域名作 cert_domain,
    // 使其也落成 direct node_domain。否则只签了证书却没建直连档,UI 建不出"域名直连"入口
    // ——真机 CF 实测发现的独立缺陷(与 public_host 推导无关)。
    let cert_domain = optional_request_text(body.cert_domain, "cert_domain", 255)?
        .or_else(|| tls_cert_domains.first().cloned());
    let cf_enabled = body.cf_enabled;
    let cf_domain = optional_request_text(body.cf_domain, "cf_domain", 255)?;
    let ip_direct_address =
        optional_request_text(body.ip_direct_address, "ip_direct_address", 255)?;
    // CF API Token 仅在请求期流转,稍后注入 agent env;此处只裁剪空白,绝不进节点创建字段。
    let cf_api_token = optional_request_text(body.cf_api_token, "cf_api_token", 4096)?;
    if cf_enabled {
        if let Some(domain) = cert_domain.as_deref().map(str::trim) {
            if !domain.is_empty() && !tls_cert_domains.iter().any(|existing| existing == domain) {
                tls_cert_domains.push(domain.to_string());
            }
        }
    }
    if !tls_cert_domains.is_empty() && acme_email.is_empty() {
        return Err("自动安装 SSL 证书时 acme_email 不能为空".to_string());
    }
    let expected_listen_ports = validate_expected_listen_ports(body.expected_listen_ports)?;
    let _client_force_reinstall = body.force_reinstall;

    let install = AgentInstallGuideResolved {
        guide_id: Uuid::new_v4(),
        access_node_id: None,
        access_node_name: name.clone(),
        control_plane_url,
        deploy_artifact_token,
        acme_email: acme_email.clone(),
        install_dir: optional_request_text(body.install_dir, "install_dir", 512)?
            .unwrap_or_else(|| "/opt/xrayc/access-agent".to_string()),
        compose_project: optional_request_text(body.compose_project, "compose_project", 128)?
            .unwrap_or_else(|| "xrayc-access".to_string()),
        xray_api_server: optional_request_text(body.xray_api_server, "xray_api_server", 128)?
            .unwrap_or_else(|| "127.0.0.1:10085".to_string()),
        xray_api_listen_host: optional_request_text(
            body.xray_api_listen_host,
            "xray_api_listen_host",
            128,
        )?
        .unwrap_or_else(|| "127.0.0.1".to_string()),
        xray_api_listen_port: require_nonzero_port(
            body.xray_api_listen_port.unwrap_or(10085),
            "xray_api_listen_port",
        )?,
        tls_cert_domains,
        heartbeat_interval_seconds: body.heartbeat_interval_seconds.unwrap_or(30),
        traffic_interval_seconds: body.traffic_interval_seconds.unwrap_or(60),
        session_idle_seconds: body.session_idle_seconds.unwrap_or(180),
        expected_listen_ports,
        disable_legacy_systemd_units: body.disable_legacy_systemd_units,
        force_reinstall: true,
        cf_api_token,
        // CF 域名非空即按 dns01 给 CF 域名签自己证书,注入 agent env 供部署脚本触发 DNS-01。
        cf_cert_mode: if cf_domain
            .as_deref()
            .map(str::trim)
            .is_some_and(|domain| !domain.is_empty())
        {
            "dns01".to_string()
        } else {
            "reuse_direct".to_string()
        },
        cf_domain: cf_domain.clone(),
    };

    Ok(OneClickAgentInstallResolved {
        install,
        name,
        public_host,
        public_port,
        remark,
        cert_domain,
        acme_email,
        cf_enabled,
        cf_domain,
        ip_direct_address,
        ssh_host,
        ssh_port,
        ssh_user,
        ssh_password,
        ssh_private_key,
    })
}

pub(crate) fn agent_install_envs(
    request: &AgentInstallGuideResolved,
    deployment_task_id: &str,
    deployment_task_report_token: &str,
) -> Vec<(String, String)> {
    let mut envs = vec![
        (
            "XRAYC_CONTROL_PLANE_URL".to_string(),
            request.control_plane_url.clone(),
        ),
        (
            "XRAYC_DEPLOY_ARTIFACT_TOKEN".to_string(),
            request.deploy_artifact_token.clone(),
        ),
        (
            "XRAYC_DEPLOY_TASK_ID".to_string(),
            deployment_task_id.to_string(),
        ),
        (
            "XRAYC_DEPLOY_TASK_REPORT_TOKEN".to_string(),
            deployment_task_report_token.to_string(),
        ),
        ("XRAYC_INSTALL_DIR".to_string(), request.install_dir.clone()),
        (
            "XRAYC_COMPOSE_PROJECT_NAME".to_string(),
            request.compose_project.clone(),
        ),
        (
            "XRAYC_XRAY_API_SERVER".to_string(),
            request.xray_api_server.clone(),
        ),
        (
            "XRAYC_XRAY_API_LISTEN_HOST".to_string(),
            request.xray_api_listen_host.clone(),
        ),
        (
            "XRAYC_XRAY_API_LISTEN_PORT".to_string(),
            request.xray_api_listen_port.to_string(),
        ),
        (
            "XRAYC_XRAY_CONFIG_PATH".to_string(),
            "/etc/xray/config.json".to_string(),
        ),
        (
            "XRAYC_XRAY_BINARY".to_string(),
            "/usr/local/bin/xrayc-xray".to_string(),
        ),
        // BUG-E 补全(对照部署脚本 651819f):绝不注入 XRAYC_XRAY_TEST_COMMAND。该命令含运行时
        // $XRAYC_XRAY_CONFIG,一旦由控制面 set 成非空,部署脚本 `if [[ -n ... ]]` 守卫就会把它写进
        // env_file,docker-compose v2 加载时把未定义的 $XRAYC_XRAY_CONFIG 插值成空 → agent 拿到
        // -config "" → 每次 xray -test 失败(open : no such file)→ 退回空配置 → 节点永不收敛
        // (compose v1/无插件节点恰好不插值才掩盖了此 bug)。改由 agent config.rs 内置等价默认接管,
        // 由 agent 自身 sh -c 在运行时展开 $XRAYC_XRAY_CONFIG(不经 compose 插值)——正是 651819f 的本意。
        (
            "XRAYC_TLS_CERT_DOMAINS".to_string(),
            request.tls_cert_domains.join(" "),
        ),
        (
            "XRAYC_TLS_CERT_EMAIL".to_string(),
            request.acme_email.clone(),
        ),
        (
            "XRAYC_HEARTBEAT_INTERVAL_SECONDS".to_string(),
            request.heartbeat_interval_seconds.to_string(),
        ),
        (
            "XRAYC_TRAFFIC_INTERVAL_SECONDS".to_string(),
            request.traffic_interval_seconds.to_string(),
        ),
        (
            "XRAYC_SESSION_IDLE_SECONDS".to_string(),
            request.session_idle_seconds.to_string(),
        ),
        (
            "XRAYC_EXPECTED_LISTEN_PORTS".to_string(),
            request
                .expected_listen_ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        (
            "XRAYC_DISABLE_LEGACY_SYSTEMD_UNITS".to_string(),
            request.disable_legacy_systemd_units.to_string(),
        ),
    ];
    if request.force_reinstall {
        envs.push((
            "XRAYC_DEPLOY_OVERWRITE_RUNTIME_CONFIG".to_string(),
            "true".to_string(),
        ));
    }
    // CF API Token 仅在请求期注入安装环境(写节点本机 0600 ini 供 certbot DNS-01);
    // 绝不进 AdminAccessNodeInput、绝不落库、不入审计/日志/回显(对齐 SSH 凭据红线)。
    if let Some(token) = request
        .cf_api_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        envs.push(("XRAYC_CLOUDFLARE_API_TOKEN".to_string(), token.to_string()));
    }
    // CF 域名与证书模式:部署脚本 dns-cloudflare 分支据此触发(cf_cert_mode=dns01 且 cf_domain 非空)。
    // 这两项非敏感(域名 + 模式枚举),可入 env 文件,与 token(只写 0600 ini)不同。
    if let Some(cf_domain) = request
        .cf_domain
        .as_deref()
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
    {
        envs.push(("XRAYC_CF_DOMAIN".to_string(), cf_domain.to_string()));
        envs.push((
            "XRAYC_CF_CERT_MODE".to_string(),
            request.cf_cert_mode.clone(),
        ));
    }
    envs
}

pub(crate) fn env_file_text(envs: &[(String, String)]) -> String {
    envs.iter()
        .map(|(key, value)| format!("{key}={}", shell_quote(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn install_command_text(environment_text: &str) -> String {
    format!(
        r#"cat > node.env <<'EOF'
{environment_text}
EOF
chmod 600 node.env
set -a
. ./node.env
set +a
bash ./deploy-access-agent.sh"#
    )
}

pub(crate) fn install_steps_json(install_command: &str) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "title": "准备服务器",
            "detail": "在目标服务器安装 Docker 和 Docker Compose，并确认 443/自定义入口端口、防火墙和域名解析已就绪。"
        }),
        serde_json::json!({
            "title": "上传安装脚本",
            "detail": "从项目根目录复制 scripts/deploy-access-agent.sh 和 scripts/lib/deploy-access-agent/ 到服务器同一个目录。"
        }),
        serde_json::json!({
            "title": "执行安装命令",
            "detail": "把 XRAYC_DEPLOY_ARTIFACT_TOKEN 替换为制品下载鉴权码，然后在服务器上的安装脚本目录执行命令。安装成功后脚本会输出节点鉴权码。",
            "command": install_command
        }),
        serde_json::json!({
            "title": "回到后台新增中转节点",
            "detail": "复制服务器输出的节点鉴权码，回到平台新增中转节点并填写名称、地址、端口、鉴权码和备注；添加成功后 agent 会自动通过心跳并拉取配置。"
        }),
    ]
}

pub(crate) fn extract_installed_agent_auth_code(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (_, tail) = line.split_once("xrayc-agent-v1:")?;
        let value = format!("xrayc-agent-v1:{tail}");
        let token = value
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches(|ch| matches!(ch, '`' | '"' | '\'' | '，' | ',' | '。' | ';'));
        if token.is_empty() {
            None
        } else {
            Some(token.to_string())
        }
    })
}

pub(crate) fn extract_installed_agent_auth_code_from_install_output(
    stdout: &str,
    stderr: &str,
) -> Option<String> {
    extract_installed_agent_auth_code(stdout).or_else(|| extract_installed_agent_auth_code(stderr))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
