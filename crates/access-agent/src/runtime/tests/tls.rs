//! 本模块测试 access-agent 的 SSL 证书状态采集和续期命令。
//! 测试只检查目标收集、日期解析、缺失证书分类和 certbot 命令形状。
//! 单元测试不执行真实 certbot，也不访问公网 ACME。
//! 上报内容必须只包含域名和脱敏摘要，不包含本机证书路径或私钥路径。
//! 续期按钮对应的命令不得默认强制续期，避免触发 ACME 频率限制。
//! 有效证书解析使用 openssl 标准 dates 输出样例。
//! 证书目标同时来自安装环境变量和当前已应用配置。
//! 修改本文件时继续保持测试不依赖真实服务器。
//! 真实续期由后续远端 E2E 覆盖。
//! 本头部满足前十行中文注释约束。

use tempfile::tempdir;
use xrayc_xray_config::{LocalExitProtocol, LocalExitService};

use super::super::tls::cert_plan::build_certbot_http01_args;
use super::super::tls::{
    build_cert_plan, build_certbot_dns01_args, build_certbot_renew_args, certbot_args_for_domain,
    certbot_args_for_plan, collect_tls_certificates, collect_tls_targets, dns01_cert_target,
    format_sign_summary, parse_openssl_certificate_dates, write_cloudflare_credentials_ini,
    CertChallenge,
};
use crate::client::{NodeDomain, NodeDomainKind};

use super::support::{sample_config, test_settings};

#[test]
fn test_format_sign_summary_disambiguates_valid_vs_backoff_skip() {
    // 良性:1 张证书还没到期被跳过 → 文案点明 "1 still valid, 0 awaiting"，不再含糊。
    // 用户报的 "signed 0, skipped 1" 正是这种(证书还剩 88 天),应一眼可辨良性。
    let benign = format_sign_summary(0, 1, 0);
    assert_eq!(
        benign,
        "signed 0, skipped 1 certificate(s) (1 still valid, 0 awaiting retry after prior failure)"
    );

    // 隐藏失败:1 张证书因上次签发失败仍在退避被跳过 → 文案点明 "0 still valid, 1 awaiting"，
    // 不再把"其实没签上、在退避"伪装成普通成功跳过。
    let suppressed = format_sign_summary(0, 0, 1);
    assert_eq!(
        suppressed,
        "signed 0, skipped 1 certificate(s) (0 still valid, 1 awaiting retry after prior failure)"
    );
    assert!(suppressed.contains("awaiting retry after prior failure"));

    // 混合:签 2、跳 3(2 有效 + 1 退避)→ 总数与分项都对得上。
    let mixed = format_sign_summary(2, 2, 1);
    assert_eq!(
        mixed,
        "signed 2, skipped 3 certificate(s) (2 still valid, 1 awaiting retry after prior failure)"
    );
}

#[test]
fn test_collect_tls_targets_merges_env_domains_and_active_config_paths() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    settings.tls_cert_domains = vec!["env.example.test".to_string()];
    let mut config = sample_config();
    config.access_lines[0].server_name = Some("line.example.test".to_string());
    config.access_lines[0].tls_certificate_file =
        Some("/etc/letsencrypt/live/line.example.test/fullchain.pem".to_string());
    config.local_exit_services.push(LocalExitService {
        id: "local-service".to_string(),
        tag: "local-service".to_string(),
        listen_port: 8443,
        protocol: LocalExitProtocol::Trojan,
        network: "tcp".to_string(),
        username: String::new(),
        password: "local-password".to_string(),
        uuid: String::new(),
        method: String::new(),
        security: Some("tls".to_string()),
        server_name: Some("local.example.test".to_string()),
        reality_dest: None,
        reality_private_key: None,
        reality_short_ids: Vec::new(),
        tls_certificate_file: Some(
            "/etc/letsencrypt/live/local.example.test/fullchain.pem".to_string(),
        ),
        tls_key_file: None,
    });

    let targets = collect_tls_targets(&settings, Some(&config), &[]);
    let domains = targets
        .iter()
        .map(|target| target.domain.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        domains,
        vec![
            "env.example.test",
            "line.example.test",
            "local.example.test"
        ]
    );
    assert!(targets
        .iter()
        .all(|target| target.certificate_file.ends_with("fullchain.pem")));
}

#[test]
fn test_collect_tls_targets_includes_node_domains_not_yet_in_entries() {
    // 多域名证书状态回报:PUT 新加但还没建入口的 node_domain 也应纳入证书目标,
    // 这样 certificate_report 会为它上报状态(有有效证书→valid;无→missing),
    // 控制面据此把 node_domains.cert_status 从 unknown 刷成真实状态(修 #60)。
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    settings.tls_cert_domains = vec!["env.example.test".to_string()];

    // 节点清单里有一个既不在 tls_cert_domains 也不在入口里的新域名。
    let node_domains = vec![
        NodeDomain {
            domain: "fresh.example.test".to_string(),
            kind: NodeDomainKind::Direct,
            cf_cert_mode: None,
            acme_email: None,
        },
        // 与 env 域名重复 → BTreeMap 去重,只出现一次。
        NodeDomain {
            domain: "env.example.test".to_string(),
            kind: NodeDomainKind::Direct,
            cf_cert_mode: None,
            acme_email: None,
        },
    ];

    let targets = collect_tls_targets(&settings, None, &node_domains);
    let domains = targets
        .iter()
        .map(|target| target.domain.as_str())
        .collect::<Vec<_>>();

    // 新域名纳入目标;env 去重后只出现一次。
    assert_eq!(domains, vec!["env.example.test", "fresh.example.test"]);
}

#[test]
fn test_collect_tls_certificates_reports_missing_without_leaking_path() {
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    settings.tls_cert_domains = vec!["missing.example.test".to_string()];

    let reports = collect_tls_certificates(&settings, None, &[]);

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].domain, "missing.example.test");
    assert_eq!(reports[0].status, "missing");
    assert!(!reports[0].error_summary.contains("/etc/letsencrypt"));
}

#[test]
fn test_parse_openssl_dates_marks_expiring_certificate() {
    let output = "\
notBefore=Jun  1 00:00:00 2026 GMT
notAfter=Jun 20 00:00:00 2026 GMT
";

    let report = parse_openssl_certificate_dates("relay.example.test", output, 1_781_136_000)
        .expect("dates parse");

    assert_eq!(report.domain, "relay.example.test");
    assert_eq!(report.status, "expiring");
    assert_eq!(report.days_remaining, Some(9));
    assert_eq!(report.not_after.as_deref(), Some("2026-06-20T00:00:00Z"));
}

#[test]
fn test_build_certbot_dns01_args() {
    // DNS-01 纯函数:构造 certbot certonly --dns-cloudflare 参数。
    // token 不入参(只通过 0600 ini 文件传递),命令里只出现 ini 路径、域名、邮箱。
    let args = build_certbot_dns01_args(
        "cf.example.test",
        "/etc/letsencrypt/cloudflare.ini",
        "ops@example.test",
    );

    assert!(args.contains(&"certonly".to_string()));
    assert!(args.contains(&"--dns-cloudflare".to_string()));
    assert!(args.contains(&"--dns-cloudflare-credentials".to_string()));
    assert!(args.contains(&"/etc/letsencrypt/cloudflare.ini".to_string()));
    assert!(args.contains(&"-d".to_string()));
    assert!(args.contains(&"cf.example.test".to_string()));
    assert!(args.contains(&"-m".to_string()));
    assert!(args.contains(&"ops@example.test".to_string()));
    assert!(args.contains(&"--non-interactive".to_string()));
    assert!(args.contains(&"--agree-tos".to_string()));
    // token 绝不进命令行参数(只在 ini 文件)。
    assert!(args.iter().all(|arg| !arg.contains("cf-secret-token")));
}

#[test]
fn test_dns01_only_when_mode_and_token() {
    // 只有 cf_cert_mode=dns01 且 cf_domain 非空且有 token 时才构造 DNS-01 目标;
    // reuse_direct 或缺 token / 缺域名 → None(不走 DNS-01)。
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    settings.cf_cert_mode = "dns01".to_string();
    settings.cf_domain = "cf.example.test".to_string();
    settings.cloudflare_api_token = "cf-secret-token".to_string();
    settings.acme_email = "ops@example.test".to_string();

    let target = dns01_cert_target(&settings).expect("dns01 target present");
    assert_eq!(target.domain, "cf.example.test");
    assert_eq!(target.acme_email, "ops@example.test");

    // reuse_direct → 不构造。
    let mut reuse = settings.clone();
    reuse.cf_cert_mode = "reuse_direct".to_string();
    assert!(dns01_cert_target(&reuse).is_none());

    // 缺 token → 不构造。
    let mut no_token = settings.clone();
    no_token.cloudflare_api_token = String::new();
    assert!(dns01_cert_target(&no_token).is_none());

    // 缺 cf_domain → 不构造。
    let mut no_domain = settings.clone();
    no_domain.cf_domain = String::new();
    assert!(dns01_cert_target(&no_domain).is_none());
}

fn renew_args(domain: &str) -> Vec<String> {
    vec![
        "renew".to_string(),
        "--cert-name".to_string(),
        domain.to_string(),
        "--non-interactive".to_string(),
        "--no-random-sleep-on-renew".to_string(),
    ]
}

#[test]
fn test_certbot_args_for_domain_picks_dns01_only_for_cf_domain_when_missing() {
    // CF 域名且证书尚未签出 → 走 DNS-01 certonly;非 CF 域名 → 走 renew。
    let target = super::super::tls::Dns01CertTarget {
        domain: "cf.example.test".to_string(),
        acme_email: "ops@example.test".to_string(),
    };

    // CF 域名、证书缺失 → certonly --dns-cloudflare。
    let issue = certbot_args_for_domain(Some(&target), "cf.example.test", false);
    assert!(issue.contains(&"certonly".to_string()));
    assert!(issue.contains(&"--dns-cloudflare".to_string()));
    assert!(issue.contains(&"cf.example.test".to_string()));

    // CF 域名但证书已存在 → renew(避免重复签触发限频)。
    let renew_existing = certbot_args_for_domain(Some(&target), "cf.example.test", true);
    assert_eq!(renew_existing, renew_args("cf.example.test"));

    // 非 CF 域名 → 永远 renew(灰云直连走 HTTP-01,不碰 DNS-01)。
    let other = certbot_args_for_domain(Some(&target), "direct.example.test", false);
    assert_eq!(other, renew_args("direct.example.test"));

    // 无 DNS-01 目标 → 一律 renew。
    let no_dns01 = certbot_args_for_domain(None, "cf.example.test", false);
    assert_eq!(no_dns01, renew_args("cf.example.test"));
}

#[test]
fn test_write_cloudflare_credentials_ini_is_0600_and_holds_token() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().expect("tempdir");
    let ini = dir.path().join("cloudflare.ini");

    write_cloudflare_credentials_ini(&ini, "cf-secret-token").expect("ini written");

    let content = std::fs::read_to_string(&ini).expect("read ini");
    assert!(content.contains("dns_cloudflare_api_token = cf-secret-token"));
    let mode = std::fs::metadata(&ini)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn test_build_certbot_http01_args() {
    // HTTP-01 纯函数:构造 certbot certonly --standalone 参数(直连灰云域名首签)。
    // 命令里只出现域名和邮箱,不含任何 CF token/凭据。
    let args = build_certbot_http01_args("direct.example.test", "ops@example.test");

    assert!(args.contains(&"certonly".to_string()));
    assert!(args.contains(&"--standalone".to_string()));
    assert!(args.contains(&"--non-interactive".to_string()));
    assert!(args.contains(&"--agree-tos".to_string()));
    assert!(args.contains(&"-m".to_string()));
    assert!(args.contains(&"ops@example.test".to_string()));
    assert!(args.contains(&"-d".to_string()));
    assert!(args.contains(&"direct.example.test".to_string()));
    // HTTP-01 不应出现任何 DNS-01/CF 凭据相关参数。
    assert!(args.iter().all(|arg| !arg.contains("dns-cloudflare")));
    assert!(args.iter().all(|arg| !arg.contains("cloudflare")));
}

#[test]
fn test_build_cert_plan_with_token_signs_all_via_dns01() {
    // 有 CF token 时:direct 和 cf 域名都走 DNS-01(同账号一个 token 通签,免占 80 端口)。
    // 决定每个域名用哪种 challenge 是纯函数,便于单测且不触真实 certbot。
    let node_domains = vec![
        NodeDomain {
            domain: "a.example.test".to_string(),
            kind: NodeDomainKind::Direct,
            cf_cert_mode: None,
            acme_email: Some("a@example.test".to_string()),
        },
        NodeDomain {
            domain: "b.example.test".to_string(),
            kind: NodeDomainKind::Direct,
            cf_cert_mode: None,
            acme_email: None,
        },
        NodeDomain {
            domain: "cf.example.test".to_string(),
            kind: NodeDomainKind::Cf,
            cf_cert_mode: Some("dns01".to_string()),
            acme_email: Some("cf@example.test".to_string()),
        },
    ];

    // 有 token → 三个域名全部 DNS-01。
    let plan = build_cert_plan(&node_domains, "fallback@example.test", true);

    assert_eq!(plan.len(), 3);
    assert!(plan.iter().all(|p| p.challenge == CertChallenge::Dns01));
    assert_eq!(plan[0].domain, "a.example.test");
    assert_eq!(plan[0].acme_email, "a@example.test");
    // 该域名未带邮箱 → 回退到节点级邮箱。
    assert_eq!(plan[1].acme_email, "fallback@example.test");
    assert_eq!(plan[2].domain, "cf.example.test");
    assert_eq!(plan[2].acme_email, "cf@example.test");
}

#[test]
fn test_build_cert_plan_without_token_all_fall_back_to_http01() {
    // 无 CF token 时(token-less):direct 与 cf 域名都回退 HTTP-01 standalone——cf 橙云域名经 CF :80
    // 回源签 HTTP-01、免 token,这才是「CF 就是 CF、免 token」的自动签路径。2 direct + 1 cf → 3 HTTP-01 + 0 DNS-01。
    let node_domains = vec![
        NodeDomain {
            domain: "a.example.test".to_string(),
            kind: NodeDomainKind::Direct,
            cf_cert_mode: None,
            acme_email: None,
        },
        NodeDomain {
            domain: "b.example.test".to_string(),
            kind: NodeDomainKind::Direct,
            cf_cert_mode: None,
            acme_email: None,
        },
        NodeDomain {
            domain: "cf.example.test".to_string(),
            kind: NodeDomainKind::Cf,
            cf_cert_mode: Some("dns01".to_string()),
            acme_email: None,
        },
    ];

    let plan = build_cert_plan(&node_domains, "fallback@example.test", false);

    assert_eq!(plan.len(), 3);
    assert_eq!(plan[0].challenge, CertChallenge::Http01);
    assert_eq!(plan[1].challenge, CertChallenge::Http01);
    assert_eq!(plan[2].challenge, CertChallenge::Http01);

    let http01 = plan
        .iter()
        .filter(|p| p.challenge == CertChallenge::Http01)
        .count();
    let dns01 = plan
        .iter()
        .filter(|p| p.challenge == CertChallenge::Dns01)
        .count();
    assert_eq!(http01, 3);
    assert_eq!(dns01, 0);
}

#[test]
fn test_certbot_args_for_plan_maps_challenge_to_command() {
    // 签发计划→certbot 参数:HTTP-01→standalone,DNS-01→cloudflare。DNS-01 命令只含 ini 路径与域名、不含 token。
    // 无 token → direct(及 cf)走 HTTP-01;有 token → cf 走 DNS-01。本测分两次构造,专测两种 challenge→命令映射。
    let http01_plan = build_cert_plan(
        &[NodeDomain {
            domain: "direct.example.test".to_string(),
            kind: NodeDomainKind::Direct,
            cf_cert_mode: None,
            acme_email: Some("ops@example.test".to_string()),
        }],
        "ops@example.test",
        false,
    );
    let http01 = certbot_args_for_plan(&http01_plan[0]);
    assert_eq!(
        http01,
        build_certbot_http01_args("direct.example.test", "ops@example.test")
    );

    let dns01_plan = build_cert_plan(
        &[NodeDomain {
            domain: "cf.example.test".to_string(),
            kind: NodeDomainKind::Cf,
            cf_cert_mode: Some("dns01".to_string()),
            acme_email: Some("ops@example.test".to_string()),
        }],
        "ops@example.test",
        true,
    );
    let dns01 = certbot_args_for_plan(&dns01_plan[0]);
    assert!(dns01.contains(&"--dns-cloudflare".to_string()));
    assert!(dns01.contains(&"cf.example.test".to_string()));
    assert!(dns01
        .iter()
        .all(|arg| !arg.to_lowercase().contains("token")));
}

#[test]
fn test_cert_plan_single_domain_backward_compat() {
    // 单域名节点(列表仅 1 项 direct)→ 与旧单域名 HTTP-01 行为一致:一条 HTTP-01 计划。
    let node_domains = vec![NodeDomain {
        domain: "relay.example.test".to_string(),
        kind: NodeDomainKind::Direct,
        cf_cert_mode: None,
        acme_email: None,
    }];

    // 无 token 单 direct 域名 → 一条 HTTP-01(与旧单域名 HTTP-01 行为一致)。
    let plan = build_cert_plan(&node_domains, "ops@example.test", false);

    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].domain, "relay.example.test");
    assert_eq!(plan[0].challenge, CertChallenge::Http01);
    assert_eq!(plan[0].acme_email, "ops@example.test");

    // 单个 cf 域名(无 token)→ 一条 HTTP-01(token-less 自动签,经 CF :80 回源,免单独配直连域名)。
    let cf_only = vec![NodeDomain {
        domain: "cf.example.test".to_string(),
        kind: NodeDomainKind::Cf,
        cf_cert_mode: Some("dns01".to_string()),
        acme_email: None,
    }];
    let cf_plan = build_cert_plan(&cf_only, "ops@example.test", false);
    assert_eq!(cf_plan.len(), 1);
    assert_eq!(cf_plan[0].challenge, CertChallenge::Http01);
    assert_eq!(cf_plan[0].acme_email, "ops@example.test");
}

#[test]
fn test_node_domain_deserializes_heartbeat_shape() {
    // agent 自定义 node_domains JSON 形状(不依赖 db),需与 db 心跳字段对齐:
    // 数组,每项 {domain, kind:'direct'|'cf', cf_cert_mode?, acme_email?}。
    let json = r#"[
        {"domain":"a.example.test","kind":"direct"},
        {"domain":"cf.example.test","kind":"cf","cf_cert_mode":"dns01","acme_email":"cf@example.test"}
    ]"#;
    let parsed: Vec<NodeDomain> = serde_json::from_str(json).expect("node_domains parse");

    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].domain, "a.example.test");
    assert_eq!(parsed[0].kind, NodeDomainKind::Direct);
    assert!(parsed[0].cf_cert_mode.is_none());
    assert!(parsed[0].acme_email.is_none());
    assert_eq!(parsed[1].kind, NodeDomainKind::Cf);
    assert_eq!(parsed[1].cf_cert_mode.as_deref(), Some("dns01"));
    assert_eq!(parsed[1].acme_email.as_deref(), Some("cf@example.test"));
}

#[test]
fn test_certbot_renew_args_do_not_force_renewal_by_default() {
    let args = build_certbot_renew_args("relay.example.test");

    assert_eq!(
        args,
        vec![
            "renew",
            "--cert-name",
            "relay.example.test",
            "--non-interactive",
            "--no-random-sleep-on-renew",
        ]
    );
    assert!(!args.contains(&"--force-renewal"));
}
