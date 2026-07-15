//! 入口"橙云自动识别"控制面 helper(只解析、不连接)。
//! db crate 不联网,故 dig 在 api 控制面层做:把对外地址解析成 IP 列表,
//! 再用 xrayc_db::cloudflare_ranges::is_cloudflare_ip 判定是否落 Cloudflare 段。
//! 命中 → 识别为 CF 模式:自动预填 cdn_enabled=true、cdn_provider=cloudflare(管理员后续可覆盖)。
//! 解析失败/无 A/AAAA 记录不阻断保存,仅"不自动开 CF"(保持管理员原始入参)。
//! DNS 解析用 tokio::net::lookup_host,只查地址、不建立连接,无敏感副作用。
//! 识别逻辑与网络解析分离:apply_cdn_detection 为纯函数可单测(注入已解析 IP),
//! resolve_target_ips 仅在 handler 真跑(单测不连网)。
//! 本模块不打印真实 IP/域名/凭据,日志只保留是否命中的布尔结论。
//! 本头部满足前十行中文注释约束。

use std::net::IpAddr;
use xrayc_db::cloudflare_ranges::is_cloudflare_ip;

/// 入口对外地址识别后的 CDN 预填结果(纯数据,供 handler 写回入参)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CdnDetection {
    pub(crate) cdn_enabled: bool,
    pub(crate) cdn_provider: String,
}

/// 选出用于识别的"对外地址":优先 cdn_hostname,为空再退回 listen_host。
///
/// 只有当目标是"域名"时才需要 dig;已是字面 IP 或为空则不解析(返回 None)。
/// 字面 IP 不必再解析——它就是最终对外地址,可直接判段(由调用方决定是否仍走判定)。
pub(crate) fn detection_target_host(cdn_hostname: &str, listen_host: &str) -> Option<String> {
    let host = if !cdn_hostname.trim().is_empty() {
        cdn_hostname.trim()
    } else {
        listen_host.trim()
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_string())
}

/// 纯函数:依据"已解析 IP 列表"决定是否自动置 CF 模式。
///
/// 规则(spec §4 识别即预填):
/// - 当前管理员已显式开 CDN → 尊重其入参,不覆盖(返回原值)。
/// - 否则任一解析 IP 落 Cloudflare 段 → 置 cdn_enabled=true、cdn_provider=cloudflare。
/// - 无命中(或解析为空)→ 保持原值,不自动开。
///
/// 解析失败的"空列表"在此天然表现为"不自动开",与"解析失败不阻断"一致。
pub(crate) fn apply_cdn_detection(
    current_cdn_enabled: bool,
    current_cdn_provider: &str,
    resolved_ips: &[IpAddr],
) -> CdnDetection {
    // 管理员已显式开 CDN 时按其入参走,识别只用于"未开时自动预填",不强制改写。
    if current_cdn_enabled {
        return CdnDetection {
            cdn_enabled: true,
            cdn_provider: current_cdn_provider.to_string(),
        };
    }
    let hit_cloudflare = resolved_ips.iter().copied().any(is_cloudflare_ip);
    if hit_cloudflare {
        CdnDetection {
            cdn_enabled: true,
            cdn_provider: "cloudflare".to_string(),
        }
    } else {
        CdnDetection {
            cdn_enabled: false,
            cdn_provider: current_cdn_provider.to_string(),
        }
    }
}

/// 解析对外地址的 A/AAAA 记录为 IP 列表(只解析、不连接)。
///
/// tokio::net::lookup_host 需要"host:port"形态,这里补一个占位端口(只为查地址,
/// 不会用该端口建连)。解析失败/无记录 → 返回空 Vec(由调用方按"不自动开"处理)。
pub(crate) async fn resolve_target_ips(host: &str) -> Vec<IpAddr> {
    // 已是字面 IP 的地址直接返回,无需走 DNS(也避免某些环境对纯 IP 的解析差异)。
    if let Ok(ip) = host.parse::<IpAddr>() {
        return vec![ip];
    }
    match tokio::net::lookup_host((host, 0u16)).await {
        Ok(addrs) => addrs.map(|socket| socket.ip()).collect(),
        // 解析失败不阻断保存:返回空列表 → apply_cdn_detection 不自动开 CF。
        Err(_) => Vec::new(),
    }
}

/// handler 用:解析对外地址并应用识别结果,返回 CDN 预填(解析失败安全降级为不自动开)。
pub(crate) async fn detect_cdn_for_entry(
    cdn_hostname: &str,
    listen_host: &str,
    current_cdn_enabled: bool,
    current_cdn_provider: &str,
) -> CdnDetection {
    // 管理员已显式开 CDN 时无需 dig,直接尊重入参(省一次解析)。
    if current_cdn_enabled {
        return CdnDetection {
            cdn_enabled: true,
            cdn_provider: current_cdn_provider.to_string(),
        };
    }
    let Some(host) = detection_target_host(cdn_hostname, listen_host) else {
        return CdnDetection {
            cdn_enabled: false,
            cdn_provider: current_cdn_provider.to_string(),
        };
    };
    let resolved = resolve_target_ips(&host).await;
    apply_cdn_detection(current_cdn_enabled, current_cdn_provider, &resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_cdn_sets_enabled_when_ip_in_cloudflare_range() {
        // 注入"已解析 IP"避免真连网:命中 CF 段 → 自动置 cdn_enabled + cloudflare。
        let cf_ip: IpAddr = "104.16.0.1".parse().unwrap();
        let detection = apply_cdn_detection(false, "", &[cf_ip]);
        assert!(detection.cdn_enabled, "命中 CF 段应自动开 CDN");
        assert_eq!(detection.cdn_provider, "cloudflare");
    }

    #[test]
    fn test_detect_cdn_keeps_disabled_for_non_cloudflare_ip() {
        // 非 CF 段(灰云/公网)→ 不自动开,provider 保持原值。
        let direct_ip: IpAddr = "203.0.113.10".parse().unwrap();
        let detection = apply_cdn_detection(false, "", &[direct_ip]);
        assert!(!detection.cdn_enabled, "非 CF 段不应自动开 CDN");
        assert_eq!(detection.cdn_provider, "");
    }

    #[test]
    fn test_detect_cdn_keeps_disabled_when_resolution_empty() {
        // 解析失败(空列表)不阻断、不自动开。
        let detection = apply_cdn_detection(false, "", &[]);
        assert!(!detection.cdn_enabled, "解析为空时不应自动开 CDN");
    }

    #[test]
    fn test_detect_cdn_respects_admin_explicit_enable() {
        // 管理员已显式开 CDN 时尊重入参,不被识别覆盖(即便解析为空)。
        let detection = apply_cdn_detection(true, "cloudflare", &[]);
        assert!(detection.cdn_enabled, "管理员已开 CDN 应保持开启");
        assert_eq!(detection.cdn_provider, "cloudflare");
    }

    #[test]
    fn test_detection_target_host_prefers_cdn_hostname() {
        assert_eq!(
            detection_target_host("cdn.example.test", "192.0.2.1"),
            Some("cdn.example.test".to_string())
        );
        assert_eq!(
            detection_target_host("", "node.example.test"),
            Some("node.example.test".to_string())
        );
        assert_eq!(detection_target_host("", ""), None);
    }
}
