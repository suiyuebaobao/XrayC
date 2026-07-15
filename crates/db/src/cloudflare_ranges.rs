//! Cloudflare 官方 IP 段比对(纯函数,不联网)。
//! 本模块内置 Cloudflare 官方公布的 IPv4/IPv6 回源 CIDR 常量。
//! 用于入口"橙云自动识别":控制面解析对外地址得到 IP 后,
//! 在此判定该 IP 是否落在 Cloudflare 段内(命中即橙云 CF 模式)。
//! 段表源: https://www.cloudflare.com/ips/ (随 spec §4 bundle 进控制面)。
//! 判定纯算术: IPv4 转 u32 掩码比对, IPv6 转 u128 掩码比对, 不做任何 DNS/网络访问。
//! 段表需随官方更新定期刷新; 刷新时只改本文件常量, 调用方与判定逻辑不变。
//! 本模块只负责"是否 CF IP", 不负责解析域名(解析在 api 控制面层做)。
//! db crate 不联网, 故此处只接收已解析好的 IpAddr。
//! 本头部满足前十行中文注释约束。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Cloudflare 官方 IPv4 段(CIDR: 前缀地址 + 掩码位数)。源 spec §4 / cloudflare.com/ips。
const CLOUDFLARE_IPV4_CIDRS: &[(Ipv4Addr, u8)] = &[
    (Ipv4Addr::new(173, 245, 48, 0), 20),
    (Ipv4Addr::new(103, 21, 244, 0), 22),
    (Ipv4Addr::new(103, 22, 200, 0), 22),
    (Ipv4Addr::new(103, 31, 4, 0), 22),
    (Ipv4Addr::new(141, 101, 64, 0), 18),
    (Ipv4Addr::new(108, 162, 192, 0), 18),
    (Ipv4Addr::new(190, 93, 240, 0), 20),
    (Ipv4Addr::new(188, 114, 96, 0), 20),
    (Ipv4Addr::new(197, 234, 240, 0), 22),
    (Ipv4Addr::new(198, 41, 128, 0), 17),
    (Ipv4Addr::new(162, 158, 0, 0), 15),
    (Ipv4Addr::new(104, 16, 0, 0), 13),
    (Ipv4Addr::new(104, 24, 0, 0), 14),
    (Ipv4Addr::new(172, 64, 0, 0), 13),
    (Ipv4Addr::new(131, 0, 72, 0), 22),
];

/// Cloudflare 官方 IPv6 段(CIDR: 前缀地址 + 掩码位数)。源 spec §4 / cloudflare.com/ips。
const CLOUDFLARE_IPV6_CIDRS: &[(Ipv6Addr, u8)] = &[
    (Ipv6Addr::new(0x2400, 0xcb00, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2803, 0xf800, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2405, 0xb500, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2405, 0x8100, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2a06, 0x98c0, 0, 0, 0, 0, 0, 0), 29),
    (Ipv6Addr::new(0x2c0f, 0xf248, 0, 0, 0, 0, 0, 0), 32),
];

/// 判定给定 IP 是否落在 Cloudflare 官方段内(橙云回源段)。
///
/// IPv4 用 u32 掩码、IPv6 用 u128 掩码做"前缀对齐"比对:
/// 把目标地址与网络前缀同时按掩码截断,相等即落段。
/// 纯算术、无网络访问;调用方负责先把域名解析成 IpAddr。
pub fn is_cloudflare_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let target = u32::from(v4);
            CLOUDFLARE_IPV4_CIDRS
                .iter()
                .any(|&(network, prefix)| ipv4_in_cidr(target, network, prefix))
        }
        IpAddr::V6(v6) => {
            let target = u128::from(v6);
            CLOUDFLARE_IPV6_CIDRS
                .iter()
                .any(|&(network, prefix)| ipv6_in_cidr(target, network, prefix))
        }
    }
}

/// 判定单个 IPv4(u32)是否落在指定 CIDR 内。prefix=0 视为全匹配。
fn ipv4_in_cidr(target: u32, network: Ipv4Addr, prefix: u8) -> bool {
    // prefix 取值 0..=32;为 0 时掩码为 0,任何地址都落段(此处段表无 /0,仍稳妥处理)。
    let mask: u32 = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    (target & mask) == (u32::from(network) & mask)
}

/// 判定单个 IPv6(u128)是否落在指定 CIDR 内。prefix=0 视为全匹配。
fn ipv6_in_cidr(target: u128, network: Ipv6Addr, prefix: u8) -> bool {
    // prefix 取值 0..=128;为 0 时掩码为 0(此处段表无 /0,仍稳妥处理)。
    let mask: u128 = if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    };
    (target & mask) == (u128::from(network) & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_cloudflare_ip() {
        // 落 Cloudflare 段内的真实示例 IP → true。
        // 104.16.0.1 ∈ 104.16.0.0/13;172.67.0.5 ∈ 172.64.0.0/13;
        // 188.114.96.1 ∈ 188.114.96.0/20;162.158.1.1 ∈ 162.158.0.0/15。
        for ip in ["104.16.0.1", "172.67.0.5", "188.114.96.1", "162.158.1.1"] {
            let addr: IpAddr = ip.parse().expect("有效 IPv4");
            assert!(is_cloudflare_ip(addr), "{ip} 应判定为 Cloudflare 段");
        }

        // 非 Cloudflare 段(灰云/公网示例)→ false。
        // 1.1.1.1 是 CF DNS 但不在橙云回源段,按段表判定应为 false。
        for ip in ["203.0.113.10", "8.8.8.8", "1.1.1.1"] {
            let addr: IpAddr = ip.parse().expect("有效 IPv4");
            assert!(!is_cloudflare_ip(addr), "{ip} 不应判定为 Cloudflare 段");
        }

        // IPv6: 2606:4700:: 段内 → true;公网 IPv6(2001:4860::,Google)→ false。
        let cf_v6: IpAddr = "2606:4700::1".parse().expect("有效 IPv6");
        assert!(
            is_cloudflare_ip(cf_v6),
            "2606:4700::1 应判定为 Cloudflare 段"
        );
        let non_cf_v6: IpAddr = "2001:4860:4860::8888".parse().expect("有效 IPv6");
        assert!(
            !is_cloudflare_ip(non_cf_v6),
            "2001:4860:4860::8888 不应判定为 Cloudflare 段"
        );
    }
}
