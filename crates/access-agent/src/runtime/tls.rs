//! 本模块采集中转节点本机 SSL 证书状态并执行受控续期任务。
//! 运行边界是 access-agent 所在服务器；控制面只下发续期任务 ID 和域名。
//! 这里不接收任意 shell，不上报证书正文、私钥路径或 certbot 原始日志。
//! 证书状态从安装环境变量和当前已应用 Xray 配置合并得到。
//! 续期使用 certbot 的安全 renew 路径，默认不强制续期以避免 ACME 限频。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;

use anyhow::Context;
use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use tokio::process::Command;
use tracing::warn;
use xrayc_xray_config::AccessConfig;

use crate::client::{NodeDomain, TlsCertificateReport, TlsRenewResult, TlsRenewTask};
use crate::config::AgentSettings;

// cert_plan 子模块从本文件拆出证书签发纯逻辑(challenge 决策、certbot 参数、退避状态机);
// 对 runtime 内可见,便于单测直接引用其纯函数,不经 tls.rs 再导出一层避免无用导出告警。
pub(super) mod cert_plan;

pub(super) use cert_plan::{
    build_cert_plan, build_certbot_dns01_args, certbot_args_for_plan, should_attempt_sign,
    CertChallenge, CertSignPlan, SignBackoff, CLOUDFLARE_CREDENTIALS_INI,
};

/// 按需自签的失败退避窗口(秒):某域名签发失败后,30 分钟内不对它重试,
/// 避免每次心跳(默认 30s)锤 Let's Encrypt 触发限频。
const SIGN_BACKOFF_SECS: i64 = 1800;
/// 证书续约提前量(秒,传给 `openssl x509 -checkend`):剩余有效期 < 30 天即重签,
/// 与 Let's Encrypt / certbot 默认续约窗口一致。旧值 86400(1 天)过紧——近到期才续,
/// agent 短暂离线就可能让证书失效;30 天给足重试与退避缓冲。直连与 CF 域名同一阈值。
const CERT_RENEW_BEFORE_EXPIRY_SECS: &str = "2592000";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TlsCertificateTarget {
    pub(super) domain: String,
    pub(super) certificate_file: String,
}

/// DNS-01 证书签发目标:仅当 cf_cert_mode=dns01 且有 cf_domain + token 时存在。
/// 只携带域名和 ACME 邮箱;token 不进此结构(只写本机 0600 ini),避免随结构泄露。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Dns01CertTarget {
    pub(super) domain: String,
    pub(super) acme_email: String,
}

/// 仅当 cf_cert_mode=dns01 且 cf_domain 非空且 token 非空时才返回 DNS-01 目标。
/// reuse_direct / 缺 token / 缺域名 → None,调用方据此不构造 DNS-01 命令(沿用复用直连证书)。
pub(super) fn dns01_cert_target(settings: &AgentSettings) -> Option<Dns01CertTarget> {
    if !settings.cf_cert_mode.trim().eq_ignore_ascii_case("dns01") {
        return None;
    }
    let domain = settings.cf_domain.trim();
    if domain.is_empty() || settings.cloudflare_api_token.trim().is_empty() {
        return None;
    }
    Some(Dns01CertTarget {
        domain: domain.to_string(),
        acme_email: settings.acme_email.trim().to_string(),
    })
}

pub(super) fn collect_tls_certificates(
    settings: &AgentSettings,
    active_config: Option<&AccessConfig>,
    node_domains: &[NodeDomain],
) -> Vec<TlsCertificateReport> {
    collect_tls_targets(settings, active_config, node_domains)
        .into_iter()
        .map(|target| certificate_report(settings, &target))
        .collect()
}

pub(super) fn collect_tls_targets(
    settings: &AgentSettings,
    active_config: Option<&AccessConfig>,
    node_domains: &[NodeDomain],
) -> Vec<TlsCertificateTarget> {
    let mut targets = BTreeMap::new();
    for domain in &settings.tls_cert_domains {
        insert_target(&mut targets, domain, None);
    }
    // 把节点全部 node_domains 也纳入证书目标:PUT 新加、还没建入口的域名也会被
    // certificate_report 报状态(有有效证书→valid;无→missing/invalid),
    // 控制面据此刷新 node_domains.cert_status,修 #60「多域名证书状态显示 unknown」。
    // 证书路径默认走 /etc/letsencrypt/live/<domain>/fullchain.pem;BTreeMap 已按域名去重。
    for node_domain in node_domains {
        insert_target(&mut targets, &node_domain.domain, None);
    }
    if let Some(config) = active_config {
        for line in &config.access_lines {
            let domain = line.server_name.as_deref().or_else(|| {
                line.tls_certificate_file
                    .as_deref()
                    .and_then(domain_from_letsencrypt_path)
            });
            insert_target(
                &mut targets,
                domain.unwrap_or_default(),
                line.tls_certificate_file.as_deref(),
            );
        }
        for service in &config.local_exit_services {
            let domain = service.server_name.as_deref().or_else(|| {
                service
                    .tls_certificate_file
                    .as_deref()
                    .and_then(domain_from_letsencrypt_path)
            });
            insert_target(
                &mut targets,
                domain.unwrap_or_default(),
                service.tls_certificate_file.as_deref(),
            );
        }
    }
    targets.into_values().collect()
}

fn insert_target(
    targets: &mut BTreeMap<String, TlsCertificateTarget>,
    domain: &str,
    path: Option<&str>,
) {
    let domain = domain.trim();
    if domain.is_empty() {
        return;
    }
    let certificate_file = path
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("/etc/letsencrypt/live/{domain}/fullchain.pem"));
    targets
        .entry(domain.to_string())
        .or_insert(TlsCertificateTarget {
            domain: domain.to_string(),
            certificate_file,
        });
}

fn domain_from_letsencrypt_path(path: &str) -> Option<&str> {
    path.strip_prefix("/etc/letsencrypt/live/")
        .and_then(|rest| rest.split_once('/').map(|(domain, _)| domain))
        .filter(|domain| !domain.trim().is_empty())
}

fn certificate_report(
    settings: &AgentSettings,
    target: &TlsCertificateTarget,
) -> TlsCertificateReport {
    if !Path::new(&target.certificate_file).is_file() {
        return TlsCertificateReport {
            domain: target.domain.clone(),
            status: "missing".to_string(),
            not_before: None,
            not_after: None,
            days_remaining: None,
            error_summary: "certificate file is missing".to_string(),
        };
    }

    match std::process::Command::new(&settings.openssl_binary)
        .args(["x509", "-noout", "-dates", "-in", &target.certificate_file])
        .output()
    {
        Ok(output) if output.status.success() => {
            let dates = String::from_utf8_lossy(&output.stdout);
            parse_openssl_certificate_dates(&target.domain, &dates, Utc::now().timestamp())
                .unwrap_or_else(|error| invalid_certificate_report(&target.domain, error))
        }
        Ok(_) => invalid_certificate_report(&target.domain, "openssl failed"),
        Err(error) => invalid_certificate_report(&target.domain, error.to_string()),
    }
}

fn invalid_certificate_report(domain: &str, error: impl AsRef<str>) -> TlsCertificateReport {
    TlsCertificateReport {
        domain: domain.to_string(),
        status: "invalid".to_string(),
        not_before: None,
        not_after: None,
        days_remaining: None,
        error_summary: truncate_summary(error.as_ref()),
    }
}

pub(super) fn parse_openssl_certificate_dates(
    domain: &str,
    output: &str,
    now_unix: i64,
) -> Result<TlsCertificateReport, String> {
    let not_before = extract_date(output, "notBefore=")?;
    let not_after = extract_date(output, "notAfter=")?;
    let not_before_time = parse_openssl_date(not_before)?;
    let not_after_time = parse_openssl_date(not_after)?;
    let days_remaining = ((not_after_time.timestamp() - now_unix) / 86_400).max(-1);
    let status = if days_remaining < 0 {
        "expired"
    } else if days_remaining <= 30 {
        "expiring"
    } else {
        "valid"
    };

    Ok(TlsCertificateReport {
        domain: domain.to_string(),
        status: status.to_string(),
        not_before: Some(format_utc(not_before_time)),
        not_after: Some(format_utc(not_after_time)),
        days_remaining: Some(days_remaining),
        error_summary: String::new(),
    })
}

fn extract_date<'a>(output: &'a str, prefix: &str) -> Result<&'a str, String> {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing {prefix}"))
}

fn parse_openssl_date(value: &str) -> Result<DateTime<Utc>, String> {
    let parsed = NaiveDateTime::parse_from_str(value, "%b %e %H:%M:%S %Y GMT")
        .map_err(|error| error.to_string())?;
    Ok(Utc.from_utc_datetime(&parsed))
}

fn format_utc(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub(super) async fn execute_tls_renew_task(
    settings: &AgentSettings,
    task: TlsRenewTask,
    node_domains: &[NodeDomain],
    backoff: &mut SignBackoff,
) -> anyhow::Result<TlsRenewResult> {
    let dns01 = dns01_cert_target(settings);
    // 节点多域名清单非空 → 走「遍历清单逐域名签发」路径。
    // 有 CF token 时 direct+cf 都 DNS-01(免端口),无 token 时 direct 回退 HTTP-01。
    // 续期任务与心跳常规处理共用同一签发逻辑(execute_node_domains_sign),
    // 只是把 request_id 透传以便回报。
    if !node_domains.is_empty() {
        return execute_node_domains_sign(settings, node_domains, backoff, &task.request_id).await;
    }

    // 向后兼容:清单为空时沿用旧单域名 renew 路径(env 单 cert_domain + 单 cf_domain)。
    // 仅当本节点要走 DNS-01(cf_cert_mode=dns01 且有 cf_domain + token)时,
    // 在续期/签发前把 token 写进本机 0600 ini,供 certbot-dns-cloudflare 复用。
    // token 只落本机文件,不进日志/不回显;写失败不阻断其余域名 renew。
    if let Some(target) = dns01.as_ref() {
        if let Err(error) = write_cloudflare_credentials_ini(
            Path::new(CLOUDFLARE_CREDENTIALS_INI),
            &settings.cloudflare_api_token,
        ) {
            warn!(%error, domain = %target.domain, "prepare cloudflare credentials ini failed");
        }
    }

    let mut failures = Vec::new();
    let mut success_count = 0usize;
    for domain in task.domains {
        // CF 域名(dns01)且证书尚未签出 → certonly DNS-01 首签;否则 renew。
        let cert_exists =
            Path::new(&format!("/etc/letsencrypt/live/{domain}/fullchain.pem")).is_file();
        let args = certbot_args_for_domain(dns01.as_ref(), &domain, cert_exists);
        let status = Command::new(&settings.certbot_binary)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .with_context(|| "run certbot")?;
        if status.success() {
            success_count += 1;
        } else {
            failures.push(domain);
        }
    }
    if failures.is_empty() {
        reload_xray(settings).await?;
        Ok(TlsRenewResult {
            request_id: task.request_id,
            status: "success".to_string(),
            message: format!("renewed {success_count} certificate(s)"),
        })
    } else {
        Ok(TlsRenewResult {
            request_id: task.request_id,
            status: "failed".to_string(),
            message: format!("failed certificate domain count: {}", failures.len()),
        })
    }
}

/// 把 CF token 写进本机 0600 ini,供 certbot-dns-cloudflare 复用;写失败只告警不阻断。
/// 多个 cf 域名同账号复用同一 ini,无需逐域名重写。token 不进日志/不回显。
fn prepare_cloudflare_credentials_ini(settings: &AgentSettings) {
    if let Err(error) = write_cloudflare_credentials_ini(
        Path::new(CLOUDFLARE_CREDENTIALS_INI),
        &settings.cloudflare_api_token,
    ) {
        warn!(%error, "prepare cloudflare credentials ini failed");
    }
}

/// 按需自签入口:agent 拿到心跳 node_domains 后调它,对清单逐域名核验+签缺的。
/// 有 CF token → direct 与 cf 域名都 DNS-01(免占 80 端口);无 token → direct 回退 HTTP-01。
/// 只要计划里出现任一 DNS-01 且有 token,就先写本机 0600 ini 注入 token(同账号复用一份)。
/// token 只落本机文件,不进日志/不回显。签发受 backoff 退避控制(见 execute_cert_plan)。
pub(super) async fn execute_node_domains_sign(
    settings: &AgentSettings,
    node_domains: &[NodeDomain],
    backoff: &mut SignBackoff,
    request_id: &str,
) -> anyhow::Result<TlsRenewResult> {
    let has_cloudflare_token = !settings.cloudflare_api_token.trim().is_empty();
    let plan = build_cert_plan(
        node_domains,
        settings.acme_email.trim(),
        has_cloudflare_token,
    );
    // 任一 DNS-01 域名且有 token → 写 0600 ini 供 certbot-dns-cloudflare 复用。
    let needs_cloudflare_ini = has_cloudflare_token
        && plan
            .iter()
            .any(|item| item.challenge == CertChallenge::Dns01);
    if needs_cloudflare_ini {
        prepare_cloudflare_credentials_ini(settings);
    }
    let now_unix = Utc::now().timestamp();
    execute_cert_plan(settings, request_id, &plan, backoff, now_unix).await
}

/// 遍历证书签发计划逐域名签发:每个域名先 `x509 -checkend` 判活,
/// 剩余有效期 >30 天即跳过(不撞 Let's Encrypt 限流);否则若不在失败退避窗口内才按 plan 重签。
/// 签发成功清退避并计 signed,失败记退避(短期内不再重试同域名)并计 failures,
/// 但不阻断其余域名;有任一新签发才 reload Xray。
async fn execute_cert_plan(
    settings: &AgentSettings,
    request_id: &str,
    plan: &[CertSignPlan],
    backoff: &mut SignBackoff,
    now_unix: i64,
) -> anyhow::Result<TlsRenewResult> {
    let mut failures = Vec::new();
    let mut signed_count = 0usize;
    // 跳过分两类、分别计数以消歧:still_valid=证书没到期(良性,真没事);
    // backoff=上次签发失败仍在退避窗口内被压住(隐藏的失败,不是"成功没事")。
    // 两者旧实现都并进一个 skipped_count、共用同一句 success 文案,会把退避失败伪装成成功。
    let mut skipped_valid = 0usize;
    let mut skipped_backoff = 0usize;
    for item in plan {
        if certificate_still_valid(settings, &item.domain) {
            skipped_valid += 1;
            continue;
        }
        // 上次签发失败且仍在退避窗口内 → 跳过,避免每次心跳锤 Let's Encrypt 触发限频。
        if !should_attempt_sign(backoff, &item.domain, now_unix, SIGN_BACKOFF_SECS) {
            skipped_backoff += 1;
            continue;
        }
        let args = certbot_args_for_plan(item);
        let status = Command::new(&settings.certbot_binary)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .with_context(|| "run certbot")?;
        if status.success() {
            backoff.record_success(&item.domain);
            signed_count += 1;
        } else {
            // 记失败时刻进入退避;短期内不对同域名重试。域名不进日志,不回显凭据。
            backoff.record_failure(&item.domain, now_unix);
            failures.push(item.domain.clone());
        }
    }
    if failures.is_empty() {
        // 有新签发才 reload;全部跳过(都还有效)则无需重载 Xray。
        if signed_count > 0 {
            reload_xray(settings).await?;
        }
        Ok(TlsRenewResult {
            request_id: request_id.to_string(),
            status: "success".to_string(),
            message: format_sign_summary(signed_count, skipped_valid, skipped_backoff),
        })
    } else {
        Ok(TlsRenewResult {
            request_id: request_id.to_string(),
            status: "failed".to_string(),
            message: format!("failed certificate domain count: {}", failures.len()),
        })
    }
}

/// 把签发/跳过计数拼成「消歧」续约文案:把旧的单一 skipped 拆成两类显式列出——
/// still_valid=证书没到期(良性、真没事),awaiting retry after prior failure=上次签发失败仍在退避窗口被压住
/// (隐藏失败,绝不是"成功没事")。这样面板 `signed 0, skipped 1 (1 still valid, 0 awaiting ...)` 一眼可辨良性,
/// 而 `(0 still valid, 1 awaiting ...)` 一眼暴露"其实没签上、在退避"。抽成纯函数便于单测,不依赖真实副作用。
pub(super) fn format_sign_summary(
    signed_count: usize,
    skipped_valid: usize,
    skipped_backoff: usize,
) -> String {
    let skipped_count = skipped_valid + skipped_backoff;
    format!(
        "signed {signed_count}, skipped {skipped_count} certificate(s) \
         ({skipped_valid} still valid, {skipped_backoff} awaiting retry after prior failure)"
    )
}

/// 用 `openssl x509 -checkend` 判断域名证书剩余有效期是否 > CERT_RENEW_BEFORE_EXPIRY_SECS（30 天）；
/// 不足即视为"需续约"返回 false，交签发流程按 build_cert_plan 的 challenge 重签（直连/CF 都 HTTP-01、免 token）。
/// 证书文件不存在 → 视为无效(需首签)。沿用 deploy 脚本的"有效即跳过"保留逻辑。
fn certificate_still_valid(settings: &AgentSettings, domain: &str) -> bool {
    let cert_path = format!("/etc/letsencrypt/live/{domain}/fullchain.pem");
    if !Path::new(&cert_path).is_file() {
        return false;
    }
    std::process::Command::new(&settings.openssl_binary)
        .args([
            "x509",
            "-checkend",
            CERT_RENEW_BEFORE_EXPIRY_SECS,
            "-noout",
            "-in",
            &cert_path,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// 按域名选择 certbot 命令:只有"DNS-01 目标域名 + 证书尚未签出"才走 DNS-01 certonly 首签,
/// 其余(已签出的 CF 域名、所有非 CF 域名、无 DNS-01 目标)一律 renew。
/// 灰云直连域名不在此走 DNS-01(它们由部署脚本 HTTP-01 签发后正常 renew)。
pub(super) fn certbot_args_for_domain(
    dns01: Option<&Dns01CertTarget>,
    domain: &str,
    cert_exists: bool,
) -> Vec<String> {
    match dns01 {
        Some(target) if target.domain == domain && !cert_exists => {
            build_certbot_dns01_args(domain, CLOUDFLARE_CREDENTIALS_INI, &target.acme_email)
        }
        _ => build_certbot_renew_args(domain)
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
    }
}

/// 把 CF API Token 写进 certbot-dns-cloudflare 凭据 ini,权限锁定 0600。
/// 内容只有 `dns_cloudflare_api_token = <token>`;token 绝不进日志/审计/心跳/回显。
/// 先以 0600 创建文件再写入,避免短暂的宽权限窗口泄露凭据。
pub(super) fn write_cloudflare_credentials_ini(path: &Path, token: &str) -> anyhow::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| "create cloudflare ini directory")?;
    }
    let mut file = open_owner_only(path)?;
    writeln!(file, "dns_cloudflare_api_token = {token}")
        .with_context(|| "write cloudflare credentials ini")?;
    Ok(())
}

#[cfg(unix)]
fn open_owner_only(path: &Path) -> anyhow::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| "open cloudflare credentials ini")
}

#[cfg(not(unix))]
fn open_owner_only(path: &Path) -> anyhow::Result<std::fs::File> {
    std::fs::File::create(path).with_context(|| "open cloudflare credentials ini")
}

pub(super) fn build_certbot_renew_args(domain: &str) -> Vec<&str> {
    vec![
        "renew",
        "--cert-name",
        domain,
        "--non-interactive",
        "--no-random-sleep-on-renew",
    ]
}

async fn reload_xray(settings: &AgentSettings) -> anyhow::Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(&settings.xray_reload_command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .with_context(|| "reload xray after certificate renewal")?;
    if !status.success() {
        anyhow::bail!("reload xray after certificate renewal failed");
    }
    Ok(())
}

fn truncate_summary(value: &str) -> String {
    value.chars().take(200).collect()
}
