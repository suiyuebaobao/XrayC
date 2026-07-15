//! 订阅 Clash/mihomo 的 `dns:` 段生成。
//! 红线(真机 bug):订阅不带 dns 段时,手机 FLClash(TUN/VPN)会回落系统/运营商 DNS,
//! 被墙域名(Telegram 等)被 DNS 污染 → 解析到假/封锁 IP → 连不上;桌面客户端有自带 DNS
//! 兜底故正常,所以表现为"PC 通、手机断、只断部分被墙站"。故内置 fake-ip + 抗污染 fallback 的
//! dns 段:国内域名走国内 DNS,被墙域名(geoip 非 CN / geosite gfw)改由国外 fallback DNS 解析,
//! fake-ip 保证按域名分流。数值均为公开 DNS,不含任何私密信息。
//! 本头部满足前十行中文注释约束。

use serde::Serialize;

/// 内置国内公共 DNS(bootstrap / 代理服务器域名 / 国内域名解析共用)。
const DOMESTIC_NAMESERVERS: [&str; 3] = ["223.5.5.5", "119.29.29.29", "114.114.114.114"];

/// Clash/mihomo `dns:` 段。字段顺序即 YAML 输出顺序(顺序不影响语义)。
#[derive(Debug, Serialize)]
pub(super) struct ClashDns {
    pub enable: bool,
    pub ipv6: bool,
    #[serde(rename = "enhanced-mode")]
    pub enhanced_mode: String,
    #[serde(rename = "fake-ip-range")]
    pub fake_ip_range: String,
    #[serde(rename = "fake-ip-filter")]
    pub fake_ip_filter: Vec<String>,
    #[serde(rename = "use-hosts")]
    pub use_hosts: bool,
    #[serde(rename = "respect-rules")]
    pub respect_rules: bool,
    #[serde(rename = "default-nameserver")]
    pub default_nameserver: Vec<String>,
    #[serde(rename = "proxy-server-nameserver")]
    pub proxy_server_nameserver: Vec<String>,
    pub nameserver: Vec<String>,
    pub fallback: Vec<String>,
    #[serde(rename = "fallback-filter")]
    pub fallback_filter: ClashDnsFallbackFilter,
}

/// fallback-filter:判定哪些解析结果改用 fallback(国外)DNS——抗污染核心。
/// 命中(结果 IP 非 CN / gfw 名单 / 指定域名)即用国外 DNS 的干净结果。
#[derive(Debug, Serialize)]
pub(super) struct ClashDnsFallbackFilter {
    pub geoip: bool,
    #[serde(rename = "geoip-code")]
    pub geoip_code: String,
    pub geosite: Vec<String>,
    pub ipcidr: Vec<String>,
    pub domain: Vec<String>,
}

/// 把静态字符串数组转成 `Vec<String>`(供各 nameserver / 过滤列表复用)。
fn to_strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

/// 内置默认 dns 配置(参照实测可用的商用订阅模板)。
/// 国内域名走 nameserver 国内 DNS(快);被墙域名(geoip 非 CN / geosite gfw / 指定域名)
/// 走 fallback 国外 DNS 抗污染;fake-ip 保证按域名分流;respect-rules 让 DNS 查询也经代理发出。
pub(super) fn default_clash_dns() -> ClashDns {
    ClashDns {
        enable: true,
        ipv6: true,
        enhanced_mode: "fake-ip".to_string(),
        fake_ip_range: "198.18.0.1/16".to_string(),
        // 本地/连通性探测域名不套 fake-ip,避免影响局域网/路由器页面/系统联网检测。
        fake_ip_filter: to_strings(&[
            "*.lan",
            "*.local",
            "+.pool.ntp.org",
            "+.msftconnecttest.com",
            "+.msftncsi.com",
            "localhost.ptlogin2.qq.com",
        ]),
        use_hosts: true,
        respect_rules: true,
        default_nameserver: to_strings(&DOMESTIC_NAMESERVERS),
        // respect-rules=true 时必须有国内 proxy-server-nameserver 解析代理服务器域名,否则连不上代理。
        proxy_server_nameserver: to_strings(&DOMESTIC_NAMESERVERS),
        nameserver: to_strings(&DOMESTIC_NAMESERVERS),
        fallback: to_strings(&["1.1.1.1", "8.8.8.8"]),
        fallback_filter: ClashDnsFallbackFilter {
            geoip: true,
            geoip_code: "CN".to_string(),
            geosite: to_strings(&["gfw"]),
            ipcidr: to_strings(&["240.0.0.0/4"]),
            domain: to_strings(&["+.google.com", "+.facebook.com", "+.youtube.com"]),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dns_uses_fake_ip_and_anti_pollution_fallback() {
        let dns = default_clash_dns();
        assert!(dns.enable);
        assert_eq!(dns.enhanced_mode, "fake-ip");
        // 必须有国外 fallback + 把被墙域名(gfw)判给 fallback,否则手机上仍被 DNS 污染。
        assert!(!dns.fallback.is_empty());
        assert_eq!(dns.fallback_filter.geoip_code, "CN");
        assert!(dns.fallback_filter.geosite.iter().any(|g| g == "gfw"));
    }

    #[test]
    fn default_dns_keeps_proxy_server_nameserver_domestic() {
        // respect-rules=true 时若无 proxy-server-nameserver 会连不上代理服务器,必须保留国内 DNS。
        let dns = default_clash_dns();
        assert!(dns.respect_rules);
        assert!(!dns.proxy_server_nameserver.is_empty());
        assert!(dns
            .proxy_server_nameserver
            .iter()
            .any(|ns| ns == "223.5.5.5"));
    }
}
