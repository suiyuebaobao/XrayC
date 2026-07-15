//! resolve_one_click_agent_install_request 的 CF 字段透传回归测试。
//! 验证一键安装请求里的 cert_domain/cf_enabled/cf_domain 正确进入 resolved,
//! 且启用 CF 时 cert_domain 并入 certbot 证书申请清单(CF 回源复用直连证书)。
//! 本测试不依赖数据库,只校验请求解析与字段透传逻辑。
//! 只使用 example.test 占位域名与测试 token,不输出任何真实部署信息。
//! 设置 DEPLOY_ARTIFACT_TOKEN 测试 env;取值与同名 env 的其它测试一致,降低并发互扰。
//! control_plane_url 显式传入,避免依赖请求头推断。
//! 断言以 resolved 输出为准,符合跨层对齐红线。
//! 文件前十行中文注释满足仓库规则。
//! 不在测试日志中输出敏感信息。

use crate::deploy_install::{
    agent_install_envs, env_file_text, resolve_one_click_agent_install_request,
};
use crate::dto::OneClickAgentInstallRequest;
use axum::http::HeaderMap;

#[tokio::test]
async fn test_resolve_one_click_threads_cf_fields_and_cert_domain() {
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    let body = OneClickAgentInstallRequest {
        name: "cf-node".to_string(),
        public_host: "node.example.test".to_string(),
        public_port: Some(443),
        remark: None,
        ssh_host: "192.0.2.10".to_string(),
        ssh_port: Some(22),
        ssh_user: "root".to_string(),
        ssh_password: Some("unit-ssh-pass".to_string()),
        ssh_private_key: None,
        control_plane_url: Some("https://panel.example.test".to_string()),
        install_dir: None,
        compose_project: None,
        xray_api_server: None,
        xray_api_listen_host: None,
        xray_api_listen_port: None,
        heartbeat_interval_seconds: None,
        traffic_interval_seconds: None,
        session_idle_seconds: None,
        expected_listen_ports: Vec::new(),
        tls_cert_domains: Vec::new(),
        acme_email: Some("ops@example.test".to_string()),
        cert_domain: Some("direct.example.test".to_string()),
        cf_enabled: true,
        cf_domain: Some("cdn.example.test".to_string()),
        ip_direct_address: None,
        cf_api_token: None,
        disable_legacy_systemd_units: false,
        force_reinstall: false,
    };
    let resolved = resolve_one_click_agent_install_request(&HeaderMap::new(), body)
        .await
        .expect("resolve should succeed");
    assert_eq!(resolved.cert_domain.as_deref(), Some("direct.example.test"));
    assert!(resolved.cf_enabled);
    assert_eq!(resolved.cf_domain.as_deref(), Some("cdn.example.test"));
    // 启用 CF 时 cert_domain 必须并入证书申请清单,certbot 安装阶段才会签出它。
    assert!(resolved
        .install
        .tls_cert_domains
        .iter()
        .any(|domain| domain == "direct.example.test"));
}

#[tokio::test]
async fn test_resolve_one_click_prefers_public_base_url_over_request_origin() {
    // 缺陷①回归:面板从非公网地址访问时前端会把 control_plane_url 设成浏览器 origin(本机/IP/备用域名),
    // 远端 agent 据此回拉制品/回连会不可达。后端必须优先用稳定的 XRAYC_PUBLIC_BASE_URL,而非请求体 origin。
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    std::env::set_var("XRAYC_PUBLIC_BASE_URL", "https://center.example.test");
    let body = OneClickAgentInstallRequest {
        name: "origin-node".to_string(),
        public_host: "node.example.test".to_string(),
        public_port: Some(443),
        remark: None,
        ssh_host: "192.0.2.10".to_string(),
        ssh_port: Some(22),
        ssh_user: "root".to_string(),
        ssh_password: Some("unit-ssh-pass".to_string()),
        ssh_private_key: None,
        // 模拟前端浏览器 origin:本机不可达地址。
        control_plane_url: Some("http://127.0.0.1:5173".to_string()),
        install_dir: None,
        compose_project: None,
        xray_api_server: None,
        xray_api_listen_host: None,
        xray_api_listen_port: None,
        heartbeat_interval_seconds: None,
        traffic_interval_seconds: None,
        session_idle_seconds: None,
        expected_listen_ports: Vec::new(),
        tls_cert_domains: Vec::new(),
        acme_email: None,
        cert_domain: None,
        cf_enabled: false,
        cf_domain: None,
        ip_direct_address: None,
        cf_api_token: None,
        disable_legacy_systemd_units: false,
        force_reinstall: false,
    };
    let resolved = resolve_one_click_agent_install_request(&HeaderMap::new(), body)
        .await
        .expect("resolve should succeed");
    assert_eq!(
        resolved.install.control_plane_url, "https://center.example.test",
        "XRAYC_PUBLIC_BASE_URL 必须优先于前端浏览器 origin"
    );
    // 注入 agent env 的 XRAYC_CONTROL_PLANE_URL 也必须是稳定中心地址。
    let envs = agent_install_envs(&resolved.install, "unit-task-id", "unit-report-token");
    let control_env = envs
        .iter()
        .find(|(key, _)| key == "XRAYC_CONTROL_PLANE_URL")
        .expect("XRAYC_CONTROL_PLANE_URL env must be present");
    assert_eq!(control_env.1, "https://center.example.test");
    std::env::remove_var("XRAYC_PUBLIC_BASE_URL");
}

#[tokio::test]
async fn test_resolve_one_click_threads_ip_direct_address() {
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    // 一键安装传入 ip_direct_address → resolved 必须带它,纯 IP 节点装完无需再单独 PUT 补。
    // 用 RFC5737 占位 IP,避免写真实地址。
    let body = OneClickAgentInstallRequest {
        name: "ip-direct-node".to_string(),
        public_host: "203.0.113.10".to_string(),
        public_port: Some(443),
        remark: None,
        ssh_host: "192.0.2.10".to_string(),
        ssh_port: Some(22),
        ssh_user: "root".to_string(),
        ssh_password: Some("unit-ssh-pass".to_string()),
        ssh_private_key: None,
        control_plane_url: Some("https://panel.example.test".to_string()),
        install_dir: None,
        compose_project: None,
        xray_api_server: None,
        xray_api_listen_host: None,
        xray_api_listen_port: None,
        heartbeat_interval_seconds: None,
        traffic_interval_seconds: None,
        session_idle_seconds: None,
        expected_listen_ports: Vec::new(),
        tls_cert_domains: Vec::new(),
        acme_email: None,
        cert_domain: None,
        cf_enabled: false,
        cf_domain: None,
        ip_direct_address: Some("203.0.113.10".to_string()),
        cf_api_token: None,
        disable_legacy_systemd_units: false,
        force_reinstall: false,
    };
    let resolved = resolve_one_click_agent_install_request(&HeaderMap::new(), body)
        .await
        .expect("resolve should succeed");
    assert_eq!(
        resolved.ip_direct_address.as_deref(),
        Some("203.0.113.10"),
        "一键安装传入的 ip_direct_address 必须透传进 resolved"
    );
}

#[tokio::test]
async fn test_resolve_one_click_derives_cert_domain_from_tls_cert_domains() {
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    // 「域名直连地址」在一键安装里是经 tls_cert_domains 传来供 agent 签证书的;调用方未单独给
    // cert_domain 时,resolved.cert_domain 必须取首个待签直连域名——否则该域名只签了证书却不落成
    // direct node_domain,UI 建不出域名直连入口(真机 CF 实测发现的独立缺陷)。
    let body = OneClickAgentInstallRequest {
        name: "direct-domain-node".to_string(),
        public_host: "203.0.113.10".to_string(),
        public_port: Some(443),
        remark: None,
        ssh_host: "192.0.2.10".to_string(),
        ssh_port: Some(22),
        ssh_user: "root".to_string(),
        ssh_password: Some("unit-ssh-pass".to_string()),
        ssh_private_key: None,
        control_plane_url: Some("https://panel.example.test".to_string()),
        install_dir: None,
        compose_project: None,
        xray_api_server: None,
        xray_api_listen_host: None,
        xray_api_listen_port: None,
        heartbeat_interval_seconds: None,
        traffic_interval_seconds: None,
        session_idle_seconds: None,
        expected_listen_ports: Vec::new(),
        tls_cert_domains: vec!["relay.example.test".to_string()],
        acme_email: Some("ops@example.test".to_string()),
        cert_domain: None,
        cf_enabled: false,
        cf_domain: None,
        ip_direct_address: Some("203.0.113.10".to_string()),
        cf_api_token: None,
        disable_legacy_systemd_units: false,
        force_reinstall: false,
    };
    let resolved = resolve_one_click_agent_install_request(&HeaderMap::new(), body)
        .await
        .expect("resolve should succeed");
    assert_eq!(
        resolved.cert_domain.as_deref(),
        Some("relay.example.test"),
        "没单独给 cert_domain 时应取首个待签直连域名,使其落成 direct node_domain"
    );
}

#[tokio::test]
async fn test_one_click_ssh_host_rejects_domain() {
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    // 一键安装的 SSH 目标必须能直连;域名(尤其橙云/CF 代理)无法直连 SSH。
    // OneClickAgentInstallRequest 未派生 Clone,这里用闭包按 ssh_host 重建请求,避免改动 DTO。
    let build = |ssh_host: &str| OneClickAgentInstallRequest {
        name: "domain-ssh-node".to_string(),
        public_host: "node.example.test".to_string(),
        public_port: Some(443),
        remark: None,
        ssh_host: ssh_host.to_string(),
        ssh_port: Some(22),
        ssh_user: "root".to_string(),
        ssh_password: Some("unit-ssh-pass".to_string()),
        ssh_private_key: None,
        control_plane_url: Some("https://panel.example.test".to_string()),
        install_dir: None,
        compose_project: None,
        xray_api_server: None,
        xray_api_listen_host: None,
        xray_api_listen_port: None,
        heartbeat_interval_seconds: None,
        traffic_interval_seconds: None,
        session_idle_seconds: None,
        expected_listen_ports: Vec::new(),
        tls_cert_domains: Vec::new(),
        acme_email: None,
        cert_domain: None,
        cf_enabled: false,
        cf_domain: None,
        ip_direct_address: None,
        cf_api_token: None,
        disable_legacy_systemd_units: false,
        force_reinstall: false,
    };

    // 占位域名作为 SSH 目标必须被直接拒绝,且消息明确指向 SSH 地址必须是 IP。
    // resolve 的 Ok 类型未派生 Debug,这里用 match 取 Err,避免对 DTO 加 Debug 派生。
    match resolve_one_click_agent_install_request(&HeaderMap::new(), build("relay.example.test"))
        .await
    {
        Ok(_) => panic!("domain ssh_host must be rejected"),
        Err(err) => assert!(
            format!("{err:?}").contains("SSH 地址必须是 IP"),
            "意外错误信息: {err:?}"
        ),
    }

    // RFC5737 占位 IP 作为 SSH 目标应当通过校验(不回归现有合法路径)。
    assert!(
        resolve_one_click_agent_install_request(&HeaderMap::new(), build("192.0.2.10"))
            .await
            .is_ok(),
        "ip ssh_host should pass validation"
    );
}

#[tokio::test]
async fn test_one_click_cf_api_token_into_env_not_db() {
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    // 用一眼可辨的占位 token,确保断言能精确定位它是否泄到节点字段。
    let cf_api_token = "unit-cf-api-token-secret-marker";
    let body = OneClickAgentInstallRequest {
        name: "cf-token-node".to_string(),
        public_host: "node.example.test".to_string(),
        public_port: Some(443),
        remark: None,
        ssh_host: "192.0.2.10".to_string(),
        ssh_port: Some(22),
        ssh_user: "root".to_string(),
        ssh_password: Some("unit-ssh-pass".to_string()),
        ssh_private_key: None,
        control_plane_url: Some("https://panel.example.test".to_string()),
        install_dir: None,
        compose_project: None,
        xray_api_server: None,
        xray_api_listen_host: None,
        xray_api_listen_port: None,
        heartbeat_interval_seconds: None,
        traffic_interval_seconds: None,
        session_idle_seconds: None,
        expected_listen_ports: Vec::new(),
        tls_cert_domains: Vec::new(),
        acme_email: Some("ops@example.test".to_string()),
        cert_domain: Some("direct.example.test".to_string()),
        cf_enabled: true,
        cf_domain: Some("cdn.example.test".to_string()),
        ip_direct_address: None,
        cf_api_token: Some(cf_api_token.to_string()),
        disable_legacy_systemd_units: false,
        force_reinstall: false,
    };
    let resolved = resolve_one_click_agent_install_request(&HeaderMap::new(), body)
        .await
        .expect("resolve should succeed");

    // cf_api_token 必须注入 agent 安装环境变量 XRAYC_CLOUDFLARE_API_TOKEN。
    let envs = agent_install_envs(&resolved.install, "unit-task-id", "unit-report-token");
    let cf_env = envs
        .iter()
        .find(|(key, _)| key == "XRAYC_CLOUDFLARE_API_TOKEN")
        .expect("XRAYC_CLOUDFLARE_API_TOKEN env must be present");
    assert_eq!(cf_env.1, cf_api_token);

    // 回归(缺陷 A):cf_domain + cf_cert_mode 也必须注入 env,否则部署脚本 DNS-01 分支永不触发。
    let cf_domain_env = envs
        .iter()
        .find(|(key, _)| key == "XRAYC_CF_DOMAIN")
        .expect("XRAYC_CF_DOMAIN env must be present");
    assert_eq!(cf_domain_env.1, "cdn.example.test");
    let cf_mode_env = envs
        .iter()
        .find(|(key, _)| key == "XRAYC_CF_CERT_MODE")
        .expect("XRAYC_CF_CERT_MODE env must be present");
    assert_eq!(cf_mode_env.1, "dns01");

    // 红线:cf_api_token 绝不进入任何会落库的节点字段(对齐 SSH 凭据红线)。
    assert_ne!(resolved.cert_domain.as_deref(), Some(cf_api_token));
    assert_ne!(resolved.cf_domain.as_deref(), Some(cf_api_token));
    assert_ne!(resolved.name, cf_api_token);
    assert_ne!(resolved.remark, cf_api_token);
    assert_ne!(resolved.public_host, cf_api_token);
    assert_ne!(resolved.acme_email, cf_api_token);
}

#[tokio::test]
async fn test_agent_install_envs_never_inject_xray_test_command() {
    // BUG-E 补全(对照部署脚本 651819f):含运行时 $XRAYC_XRAY_CONFIG 的 xray-test 命令一旦经
    // env_file 注入,compose v2 会把未定义的 $XRAYC_XRAY_CONFIG 插值成空 → agent 拿到 -config ""
    // → xray -test 每次失败 → 退回空配置 → 节点永不收敛。控制面这条无条件注入正是把该变量 set 成
    // 非空,触发部署脚本 `if [[ -n ... ]]` 守卫把它写进 env_file,绕过 651819f「默认不写」的本意。
    // 修复:agent_install_envs 不再注入 XRAYC_XRAY_TEST_COMMAND,改由 agent config.rs 内置等价默认
    // 接管(由 agent 自身 sh -c 展开 $XRAYC_XRAY_CONFIG,不经 compose 插值)。一键与手动安装共用
    // agent_install_envs,本断言同时守住两条安装路径。
    std::env::set_var("DEPLOY_ARTIFACT_TOKEN", "unit-deploy-artifact-token");
    let body = OneClickAgentInstallRequest {
        name: "no-test-cmd-node".to_string(),
        public_host: "node.example.test".to_string(),
        public_port: Some(443),
        remark: None,
        ssh_host: "192.0.2.10".to_string(),
        ssh_port: Some(22),
        ssh_user: "root".to_string(),
        ssh_password: Some("unit-ssh-pass".to_string()),
        ssh_private_key: None,
        control_plane_url: Some("https://panel.example.test".to_string()),
        install_dir: None,
        compose_project: None,
        xray_api_server: None,
        xray_api_listen_host: None,
        xray_api_listen_port: None,
        heartbeat_interval_seconds: None,
        traffic_interval_seconds: None,
        session_idle_seconds: None,
        expected_listen_ports: Vec::new(),
        tls_cert_domains: Vec::new(),
        acme_email: None,
        cert_domain: None,
        cf_enabled: false,
        cf_domain: None,
        ip_direct_address: None,
        cf_api_token: None,
        disable_legacy_systemd_units: false,
        force_reinstall: false,
    };
    let resolved = resolve_one_click_agent_install_request(&HeaderMap::new(), body)
        .await
        .expect("resolve should succeed");
    let envs = agent_install_envs(&resolved.install, "unit-task-id", "unit-report-token");
    assert!(
        !envs.iter().any(|(key, _)| key == "XRAYC_XRAY_TEST_COMMAND"),
        "agent_install_envs 绝不能注入 XRAYC_XRAY_TEST_COMMAND(BUG-E:set 该值会让部署脚本写进 \
         env_file,compose v2 把 $XRAYC_XRAY_CONFIG 插空,agent 拿到 -config \"\");\
         应交由 agent config.rs 内置默认接管"
    );
    // 同源校验:渲染出的 env_file 文本(install_command 写进 node.env 的内容)也不得带该 key。
    let env_text = env_file_text(&envs);
    assert!(
        !env_text.contains("XRAYC_XRAY_TEST_COMMAND="),
        "渲染出的安装环境文本不得出现 XRAYC_XRAY_TEST_COMMAND= 行"
    );
}
