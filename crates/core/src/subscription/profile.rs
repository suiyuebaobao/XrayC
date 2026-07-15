//! 本模块负责组装 Clash/mihomo 配置文件。
//! 它只消费已经筛选过的中转入口，并把规则目标约束到实际代理组。
//! 具体协议节点字段由代理渲染模块生成。
//! 订阅节点名称来自中转入口自身，不能被出口资源覆盖。
//! 代理节点名称和代理分组名称共享客户端命名空间，必须避免重名。
//! 客户端普通分组来自套餐授权分组，不再由订阅设置自定义。
//! 默认规则会被改写到首个实际代理分组，避免历史 PROXY 残留。
//! 自动测速分组保留管理员配置名称。
//! 普通 select 分组只输出分组名称和代理成员。
//! 本模块不得输出第三方出口地址、账号、密码或代理 URL。

use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use super::dns::{default_clash_dns, ClashDns};
use super::options::SubscriptionOptions;
use super::proxy::clash_proxy_for_line;
use super::VisibleLine;

#[derive(Debug, Serialize)]
struct ClashProfile {
    #[serde(rename = "mixed-port")]
    mixed_port: u16,
    #[serde(rename = "allow-lan")]
    allow_lan: bool,
    mode: String,
    #[serde(rename = "log-level", skip_serializing_if = "Option::is_none")]
    log_level: Option<String>,
    // 内置 fake-ip + 抗污染 fallback 的 dns 段:手机 FLClash 无此段会回落系统 DNS 被污染,
    // 被墙域名(Telegram 等)连不上;详见 subscription/dns.rs 模块头部说明。
    dns: ClashDns,
    proxies: Vec<Value>,
    #[serde(rename = "proxy-groups")]
    proxy_groups: Vec<ProxyGroup>,
    rules: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ProxyGroup {
    #[serde(skip)]
    source_group_id: Option<uuid::Uuid>,
    #[serde(skip)]
    dedicated_rules: Vec<String>,
    name: String,
    #[serde(rename = "type")]
    group_type: String,
    proxies: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interval: Option<u32>,
}

pub(super) fn render_clash_yaml(
    options: &SubscriptionOptions,
    visible: &[VisibleLine],
    default_line_group_id: Option<uuid::Uuid>,
) -> String {
    let mut seen_proxy_names = HashSet::new();
    let proxies = visible
        .iter()
        .filter(|visible_line| seen_proxy_names.insert(visible_line.proxy_name.clone()))
        .map(clash_proxy_for_line)
        .collect();
    let proxy_groups = build_proxy_groups(options, visible);
    let default_group_name = default_rule_group_name(&proxy_groups, default_line_group_id);

    let profile = ClashProfile {
        mixed_port: options.mixed_port,
        allow_lan: options.allow_lan,
        mode: options.mode.clone(),
        log_level: options.log_level.clone(),
        dns: default_clash_dns(),
        proxies,
        rules: normalize_rules(options.rules.clone(), &default_group_name, &proxy_groups),
        proxy_groups,
    };

    serde_yaml::to_string(&profile).expect("clash profile should serialize")
}

fn default_rule_group_name(
    proxy_groups: &[ProxyGroup],
    default_line_group_id: Option<uuid::Uuid>,
) -> String {
    if let Some(default_line_group_id) = default_line_group_id {
        if let Some(group) = proxy_groups.iter().find(|group| {
            group.group_type == "select" && group.source_group_id == Some(default_line_group_id)
        }) {
            return group.name.clone();
        }
    }
    proxy_groups
        .iter()
        .find(|group| group.group_type == "select")
        .or_else(|| proxy_groups.first())
        .map(|group| group.name.clone())
        .unwrap_or_else(|| "PROXY".to_string())
}

fn build_proxy_groups(options: &SubscriptionOptions, visible: &[VisibleLine]) -> Vec<ProxyGroup> {
    let mut seen_proxy_names = HashSet::new();
    let all_proxy_names = visible
        .iter()
        .filter(|visible_line| seen_proxy_names.insert(visible_line.proxy_name.clone()))
        .map(|visible_line| visible_line.proxy_name.clone())
        .collect::<Vec<_>>();
    let mut groups = default_section_groups(visible, &all_proxy_names);

    if groups.is_empty() {
        groups.push(default_proxy_group(
            "PROXY".to_string(),
            uuid::Uuid::nil(),
            Vec::new(),
            all_proxy_names.clone(),
        ));
    }
    if options.auto_test_enabled && !all_proxy_names.is_empty() {
        let mut used_names = groups
            .iter()
            .map(|group| group.name.clone())
            .chain(all_proxy_names.iter().cloned())
            .collect::<HashSet<_>>();
        groups.insert(
            0,
            ProxyGroup {
                source_group_id: None,
                dedicated_rules: Vec::new(),
                name: unique_proxy_group_name(
                    &options.auto_test_name,
                    uuid::Uuid::nil(),
                    &mut used_names,
                ),
                group_type: "url-test".to_string(),
                proxies: all_proxy_names,
                url: Some(options.auto_test_url.clone()),
                interval: Some(options.auto_test_interval_seconds),
            },
        );
    }
    groups
}

fn default_section_groups(visible: &[VisibleLine], proxy_names: &[String]) -> Vec<ProxyGroup> {
    let mut groups = Vec::<SectionGroup>::new();
    for visible_line in visible {
        let raw_name = if visible_line.section_group_name.trim().is_empty() {
            "默认分组".to_string()
        } else {
            visible_line.section_group_name.trim().to_string()
        };
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.id == visible_line.section_group_id)
        {
            if !group.proxies.contains(&visible_line.proxy_name) {
                group.proxies.push(visible_line.proxy_name.clone());
            }
        } else {
            groups.push(SectionGroup {
                id: visible_line.section_group_id,
                name: raw_name,
                dedicated_rules: visible_line.section_group_dedicated_rules.clone(),
                proxies: vec![visible_line.proxy_name.clone()],
            });
        }
    }

    let name_counts = groups
        .iter()
        .fold(HashMap::<String, usize>::new(), |mut acc, group| {
            *acc.entry(group.name.clone()).or_insert(0) += 1;
            acc
        });
    let mut used_names = proxy_names.iter().cloned().collect::<HashSet<_>>();
    groups
        .into_iter()
        .map(|group| {
            let name = if name_counts.get(&group.name).copied().unwrap_or(0) > 1 {
                format!("{} {}", group.name, short_group_id(group.id))
            } else {
                group.name
            };
            let name = unique_proxy_group_name(&name, group.id, &mut used_names);
            default_proxy_group(name, group.id, group.dedicated_rules, group.proxies)
        })
        .collect()
}

#[derive(Debug)]
struct SectionGroup {
    id: uuid::Uuid,
    name: String,
    dedicated_rules: Vec<String>,
    proxies: Vec<String>,
}

fn default_proxy_group(
    name: String,
    source_group_id: uuid::Uuid,
    dedicated_rules: Vec<String>,
    proxies: Vec<String>,
) -> ProxyGroup {
    ProxyGroup {
        source_group_id: Some(source_group_id),
        dedicated_rules,
        name,
        group_type: "select".to_string(),
        proxies,
        url: None,
        interval: None,
    }
}

fn unique_proxy_group_name(
    base_name: &str,
    id: uuid::Uuid,
    used_names: &mut HashSet<String>,
) -> String {
    let base_name = base_name.trim();
    let base_name = if base_name.is_empty() {
        "PROXY"
    } else {
        base_name
    };
    if used_names.insert(base_name.to_string()) {
        return base_name.to_string();
    }

    let grouped_name = format!("{base_name} 分组");
    if used_names.insert(grouped_name.clone()) {
        return grouped_name;
    }

    let short_name = format!("{base_name} 分组 {}", short_group_id(id));
    if used_names.insert(short_name.clone()) {
        return short_name;
    }

    let mut index = 2;
    loop {
        let candidate = format!("{base_name} 分组 {index}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

fn normalize_rules(
    mut rules: Vec<String>,
    default_group_name: &str,
    proxy_groups: &[ProxyGroup],
) -> Vec<String> {
    let mut merged_rules = Vec::new();
    for group in proxy_groups
        .iter()
        .filter(|group| group.group_type == "select" && !group.dedicated_rules.is_empty())
    {
        merged_rules.extend(
            group
                .dedicated_rules
                .iter()
                .filter_map(|rule| dedicated_rule_targeting_group(rule, &group.name)),
        );
    }

    if rules.is_empty() {
        rules = default_subscription_rules(default_group_name);
    }
    let group_names = proxy_groups
        .iter()
        .map(|group| group.name.as_str())
        .collect::<Vec<_>>();
    let mut has_match = false;
    for rule in &mut rules {
        has_match |= rule.trim().to_ascii_uppercase().starts_with("MATCH,");
        *rule = rewrite_rule_target(rule, default_group_name, &group_names);
    }
    if !has_match {
        rules.push(format!("MATCH,{default_group_name}"));
    }
    merged_rules.extend(rules);
    merged_rules
}

fn dedicated_rule_targeting_group(rule: &str, group_name: &str) -> Option<String> {
    let mut parts = rule
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 || parts[0].eq_ignore_ascii_case("MATCH") {
        return None;
    }

    let has_no_resolve = parts
        .last()
        .is_some_and(|part| part.eq_ignore_ascii_case("no-resolve"));
    if has_no_resolve {
        if parts.len() == 3 {
            parts.insert(2, group_name);
        } else if parts.len() > 3 {
            let target_index = parts.len().saturating_sub(2);
            if should_rewrite_dedicated_target(parts[target_index]) {
                parts[target_index] = group_name;
            }
        }
    } else if parts.len() == 2 {
        parts.push(group_name);
    } else {
        let target_index = parts.len() - 1;
        if should_rewrite_dedicated_target(parts[target_index]) {
            parts[target_index] = group_name;
        }
    }

    Some(parts.join(","))
}

fn should_rewrite_dedicated_target(target: &str) -> bool {
    !matches!(
        target.to_ascii_uppercase().as_str(),
        "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS"
    )
}

fn default_subscription_rules(default_group_name: &str) -> Vec<String> {
    [
        // 国外 AI 服务必须优先走代理，避免被后续中国直连规则抢先匹配。
        "DOMAIN-SUFFIX,chatgpt.com,PROXY",
        "DOMAIN-SUFFIX,chat.com,PROXY",
        "DOMAIN-SUFFIX,openai.com,PROXY",
        "DOMAIN-SUFFIX,openaiapi.com,PROXY",
        "DOMAIN-SUFFIX,oaiusercontent.com,PROXY",
        "DOMAIN-SUFFIX,oaistatic.com,PROXY",
        "DOMAIN-SUFFIX,claude.ai,PROXY",
        "DOMAIN-SUFFIX,anthropic.com,PROXY",
        "DOMAIN-SUFFIX,grok.com,PROXY",
        "DOMAIN-SUFFIX,x.ai,PROXY",
        "DOMAIN-SUFFIX,gemini.google.com,PROXY",
        "DOMAIN-SUFFIX,aistudio.google.com,PROXY",
        "DOMAIN-SUFFIX,ai.google.dev,PROXY",
        "DOMAIN-SUFFIX,generativelanguage.googleapis.com,PROXY",
        "DOMAIN-SUFFIX,makersuite.google.com,PROXY",
        "DOMAIN-SUFFIX,bard.google.com,PROXY",
        "DOMAIN-SUFFIX,perplexity.ai,PROXY",
        "DOMAIN-SUFFIX,poe.com,PROXY",
        "DOMAIN-SUFFIX,cursor.com,PROXY",
        "DOMAIN-SUFFIX,codeium.com,PROXY",
        "DOMAIN-SUFFIX,windsurf.com,PROXY",
        "DOMAIN-SUFFIX,local,DIRECT",
        "DOMAIN-SUFFIX,localhost,DIRECT",
        "DOMAIN,localhost,DIRECT",
        "IP-CIDR,127.0.0.0/8,DIRECT,no-resolve",
        "IP-CIDR,10.0.0.0/8,DIRECT,no-resolve",
        "IP-CIDR,172.16.0.0/12,DIRECT,no-resolve",
        "IP-CIDR,192.168.0.0/16,DIRECT,no-resolve",
        "IP-CIDR,100.64.0.0/10,DIRECT,no-resolve",
        "IP-CIDR,224.0.0.0/4,DIRECT,no-resolve",
        "IP-CIDR6,::1/128,DIRECT,no-resolve",
        "IP-CIDR6,fc00::/7,DIRECT,no-resolve",
        "IP-CIDR6,fe80::/10,DIRECT,no-resolve",
        "DOMAIN-SUFFIX,cn,DIRECT",
        "DOMAIN-KEYWORD,-cn,DIRECT",
        "DOMAIN-SUFFIX,qq.com,DIRECT",
        "DOMAIN-SUFFIX,weixin.qq.com,DIRECT",
        // 国内大厂图片/静态 CDN 明确直连:这些 IP 常被 geoip 误判成非 CN,
        // 只靠泛化 cn 兜底会被 DNS fallback-filter 误当污染、踢给国外 DNS,导致图片转圈加载不出。
        "DOMAIN-SUFFIX,qpic.cn,DIRECT",
        "DOMAIN-SUFFIX,qlogo.cn,DIRECT",
        "DOMAIN-SUFFIX,gtimg.com,DIRECT",
        "DOMAIN-SUFFIX,gtimg.cn,DIRECT",
        "DOMAIN-SUFFIX,hdslb.com,DIRECT",
        "DOMAIN-SUFFIX,sinaimg.cn,DIRECT",
        "DOMAIN-SUFFIX,126.net,DIRECT",
        "DOMAIN-SUFFIX,127.net,DIRECT",
        "DOMAIN-SUFFIX,pstatp.com,DIRECT",
        "DOMAIN-SUFFIX,iqiyipic.com,DIRECT",
        "DOMAIN-SUFFIX,ykimg.com,DIRECT",
        "DOMAIN-SUFFIX,baidu.com,DIRECT",
        "DOMAIN-SUFFIX,bdstatic.com,DIRECT",
        "DOMAIN-SUFFIX,taobao.com,DIRECT",
        "DOMAIN-SUFFIX,tmall.com,DIRECT",
        "DOMAIN-SUFFIX,alicdn.com,DIRECT",
        "DOMAIN-SUFFIX,alipay.com,DIRECT",
        "DOMAIN-SUFFIX,jd.com,DIRECT",
        "DOMAIN-SUFFIX,360buyimg.com,DIRECT",
        "DOMAIN-SUFFIX,bilibili.com,DIRECT",
        "DOMAIN-SUFFIX,bilivideo.com,DIRECT",
        "DOMAIN-SUFFIX,douyin.com,DIRECT",
        "DOMAIN-SUFFIX,byteimg.com,DIRECT",
        "DOMAIN-SUFFIX,ixigua.com,DIRECT",
        "DOMAIN-SUFFIX,163.com,DIRECT",
        "DOMAIN-SUFFIX,126.com,DIRECT",
        "DOMAIN-SUFFIX,netease.com,DIRECT",
        "DOMAIN-SUFFIX,meituan.com,DIRECT",
        "DOMAIN-SUFFIX,dianping.com,DIRECT",
        "DOMAIN-SUFFIX,amap.com,DIRECT",
        "DOMAIN-SUFFIX,autonavi.com,DIRECT",
        "DOMAIN-SUFFIX,mi.com,DIRECT",
        "DOMAIN-SUFFIX,xiaomi.com,DIRECT",
        "DOMAIN-SUFFIX,huawei.com,DIRECT",
        "DOMAIN-SUFFIX,zhihu.com,DIRECT",
        "DOMAIN-SUFFIX,zhimg.com,DIRECT",
        "DOMAIN-SUFFIX,weibo.com,DIRECT",
        "DOMAIN-SUFFIX,sina.com.cn,DIRECT",
        "DOMAIN-SUFFIX,csdn.net,DIRECT",
        "DOMAIN-SUFFIX,aliyun.com,DIRECT",
        "DOMAIN-SUFFIX,tencent.com,DIRECT",
        "DOMAIN-SUFFIX,qcloud.com,DIRECT",
        "GEOSITE,CN,DIRECT",
        "GEOIP,CN,DIRECT,no-resolve",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .chain(std::iter::once(format!("MATCH,{default_group_name}")))
    .collect()
}

fn rewrite_rule_target(rule: &str, default_group_name: &str, group_names: &[&str]) -> String {
    let mut parts = rule
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return rule.trim().to_string();
    }

    let is_match = parts[0].eq_ignore_ascii_case("MATCH");
    let target_index = if !is_match
        && parts.len() > 2
        && parts
            .last()
            .is_some_and(|part| part.eq_ignore_ascii_case("no-resolve"))
    {
        parts.len().saturating_sub(2)
    } else {
        parts.len() - 1
    };
    let target = parts[target_index];
    if should_rewrite_target(target, is_match, group_names) {
        parts[target_index] = default_group_name;
    }
    parts.join(",")
}

fn should_rewrite_target(target: &str, is_match: bool, group_names: &[&str]) -> bool {
    if group_names.contains(&target) {
        return false;
    }
    let upper_target = target.to_ascii_uppercase();
    if upper_target == "PROXY" {
        return true;
    }
    if is_match {
        return !matches!(upper_target.as_str(), "REJECT" | "REJECT-DROP");
    }
    !matches!(
        upper_target.as_str(),
        "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS"
    )
}

fn short_group_id(id: uuid::Uuid) -> String {
    format!("({})", id.to_string().chars().take(8).collect::<String>())
}
