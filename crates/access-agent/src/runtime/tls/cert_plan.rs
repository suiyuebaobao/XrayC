//! 本模块是 access-agent 证书签发的纯逻辑子模块,从 tls.rs 拆出以控单文件规模。
//! 职责:遍历节点 node_domains 决定每个域名的签发 challenge(HTTP-01/DNS-01),
//! 把签发计划映射成 certbot 参数,以及「按需自签的失败退避」纯状态机。
//! 运行边界:这里不触真实 certbot、不访问网络、不读写文件,只做可单测的纯决策。
//! 有 CF token 时 direct 与 cf 域名都走 DNS-01(同账号一个 token 通签、免占 80 端口);
//! 无 token 时 direct 回退 HTTP-01 standalone。token 绝不进任何结构/参数/日志。
//! 退避状态记录每域名上次签发失败时刻,退避窗口内不再对同域名锤 Let's Encrypt。
//! 本头部满足前十行中文注释约束。

use std::collections::BTreeMap;

use crate::client::{NodeDomain, NodeDomainKind};

/// CF DNS-01 凭据 ini 的固定路径(0600),供 certbot-dns-cloudflare 签发与续期复用。
pub(in crate::runtime) const CLOUDFLARE_CREDENTIALS_INI: &str = "/etc/letsencrypt/cloudflare.ini";

/// 单个域名的证书签发挑战方式:HTTP-01(直连灰云无 token)或 DNS-01(CF token 通签)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime) enum CertChallenge {
    Http01,
    Dns01,
}

/// 一条「域名→签发方式」计划:遍历 node_domains 的纯函数产物,便于单测。
/// acme_email 已在生成计划时把节点级回退值补齐,执行端无需再判空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runtime) struct CertSignPlan {
    pub(in crate::runtime) domain: String,
    pub(in crate::runtime) challenge: CertChallenge,
    pub(in crate::runtime) acme_email: String,
}

/// 纯函数:遍历节点 node_domains 清单,逐域名决定证书签发方式,生成签发计划。
/// has_cloudflare_token=true 时:direct 与 cf 域名都走 DNS-01(CF token 同账号通签,免占 80 端口最稳);
/// false 时(token-less):direct 与 cf 域名都回退 HTTP-01 standalone——CF 橙云域名的 HTTP-01 挑战经 CF :80
/// 回源到源站 certbot 应答即可签出,免 token、免单独配直连证书域名,这才是「CF 就是 CF、免 token」的自动签路径。
/// 每项缺 acme_email 时回退到节点级 fallback_acme_email。
/// 单域名节点(列表仅 1 项)→ 单条计划,行为与旧单域名一致(向后兼容)。
/// 这里只做「决定每个域名用哪种 challenge」,不触真实 certbot,便于单测且不撞 ACME 限流。
pub(in crate::runtime) fn build_cert_plan(
    node_domains: &[NodeDomain],
    fallback_acme_email: &str,
    has_cloudflare_token: bool,
) -> Vec<CertSignPlan> {
    node_domains
        .iter()
        .map(|item| {
            // direct 与 cf 域名同口径:有 CF token 走 DNS-01(同账号通签、免占 80 端口);
            // 无 token 则都回退 HTTP-01 standalone——CF 橙云域名的 HTTP-01 challenge 经 CF :80 回源到源站
            // certbot 应答即可签出(免 token、免单独配直连证书域名),这才是 token-less 正经配 CF 的自动签证书路径。
            let challenge = match item.kind {
                _ if has_cloudflare_token => CertChallenge::Dns01,
                NodeDomainKind::Direct | NodeDomainKind::Cf => CertChallenge::Http01,
            };
            let acme_email = item
                .acme_email
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(fallback_acme_email)
                .to_string();
            CertSignPlan {
                domain: item.domain.trim().to_string(),
                challenge,
                acme_email,
            }
        })
        .collect()
}

/// 纯函数:构造 `certbot certonly --standalone ...` 参数(给直连灰云域名签 HTTP-01 证书)。
/// 命令里只出现域名与邮箱,绝不含任何 CF token/DNS-01 凭据;与部署脚本 HTTP-01 形态一致。
pub(in crate::runtime) fn build_certbot_http01_args(domain: &str, acme_email: &str) -> Vec<String> {
    vec![
        "certonly".to_string(),
        "--standalone".to_string(),
        "--non-interactive".to_string(),
        "--agree-tos".to_string(),
        "-m".to_string(),
        acme_email.to_string(),
        "-d".to_string(),
        domain.to_string(),
    ]
}

/// 纯函数:构造 `certbot certonly --dns-cloudflare ...` 参数(给域名签 DNS-01 证书)。
/// token 不入参,只通过 credentials_ini_path 指向的 0600 ini 传递,命令行不含任何凭据。
pub(in crate::runtime) fn build_certbot_dns01_args(
    cf_domain: &str,
    credentials_ini_path: &str,
    acme_email: &str,
) -> Vec<String> {
    vec![
        "certonly".to_string(),
        "--dns-cloudflare".to_string(),
        "--dns-cloudflare-credentials".to_string(),
        credentials_ini_path.to_string(),
        "--non-interactive".to_string(),
        "--agree-tos".to_string(),
        "-m".to_string(),
        acme_email.to_string(),
        "-d".to_string(),
        cf_domain.to_string(),
    ]
}

/// 纯函数:把一条签发计划映射成 certbot 参数。HTTP-01→standalone,DNS-01→cloudflare。
/// DNS-01 的 token 不入参,只通过本机 0600 ini 传递;命令行不含任何凭据。
pub(in crate::runtime) fn certbot_args_for_plan(plan: &CertSignPlan) -> Vec<String> {
    match plan.challenge {
        CertChallenge::Http01 => build_certbot_http01_args(&plan.domain, &plan.acme_email),
        CertChallenge::Dns01 => {
            build_certbot_dns01_args(&plan.domain, CLOUDFLARE_CREDENTIALS_INI, &plan.acme_email)
        }
    }
}

/// 按需自签的失败退避状态:记录每个域名「上次签发失败的 unix 秒」。
/// 退避窗口内不再对同域名重试,避免每次心跳(默认 30s)锤 Let's Encrypt 触发限频。
/// 签发成功则清掉该域名记录,使下一周期到期续签不被旧失败卡住。
#[derive(Debug, Default)]
pub(in crate::runtime) struct SignBackoff {
    last_failure_unix: BTreeMap<String, i64>,
}

impl SignBackoff {
    /// 记一次该域名的签发失败时刻(unix 秒),供后续退避判定。
    pub(in crate::runtime) fn record_failure(&mut self, domain: &str, now_unix: i64) {
        self.last_failure_unix.insert(domain.to_string(), now_unix);
    }

    /// 签发成功:清掉该域名的失败记录,后续不再被旧失败的退避拦截。
    pub(in crate::runtime) fn record_success(&mut self, domain: &str) {
        self.last_failure_unix.remove(domain);
    }

    /// 读该域名上次失败时刻(无记录返回 None)。
    fn last_failure(&self, domain: &str) -> Option<i64> {
        self.last_failure_unix.get(domain).copied()
    }
}

/// 纯函数:判断现在是否允许对该域名尝试签发。无失败记录 → 允许;
/// 有失败记录且距上次失败 < backoff_secs → 退避拦截(返回 false);窗口过后放行。
/// 把退避判定抽成纯函数便于单测,不依赖真实时钟。
pub(in crate::runtime) fn should_attempt_sign(
    backoff: &SignBackoff,
    domain: &str,
    now_unix: i64,
    backoff_secs: i64,
) -> bool {
    match backoff.last_failure(domain) {
        Some(failed_at) => now_unix.saturating_sub(failed_at) >= backoff_secs,
        None => true,
    }
}
