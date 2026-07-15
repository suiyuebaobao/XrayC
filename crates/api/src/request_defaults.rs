//! 本模块集中存放请求反序列化默认值。
//! 这些函数只返回静态默认值或解析环境变量默认配置。
//! 不在这里放业务处理器，避免路由行为被拆分。
//! 保持函数名由主模块私有导入，确保反序列化默认路径不变。

pub(crate) const DEFAULT_ACCESS_TOKEN_TTL_SECONDS: i64 = 86_400;
pub(crate) const DEFAULT_REFRESH_TOKEN_TTL_SECONDS: i64 = 604_800;
pub(crate) const REFRESH_COOKIE_SECURE_ENV: &str = "XRAYC_REFRESH_COOKIE_SECURE";

const MAX_AUTH_TOKEN_TTL_SECONDS: i64 = 10 * 365 * 24 * 60 * 60;

pub(crate) fn env_duration_seconds(name: &str, default_seconds: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_duration_seconds(&value))
        .unwrap_or(default_seconds)
}

pub(crate) fn parse_duration_seconds(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (number, multiplier) = match value.as_bytes().last().copied() {
        Some(b's' | b'S') => (&value[..value.len() - 1], 1_i64),
        Some(b'm' | b'M') => (&value[..value.len() - 1], 60_i64),
        Some(b'h' | b'H') => (&value[..value.len() - 1], 60_i64 * 60),
        Some(b'd' | b'D') => (&value[..value.len() - 1], 24_i64 * 60 * 60),
        _ => (value, 1_i64),
    };
    let seconds = number.trim().parse::<i64>().ok()?.checked_mul(multiplier)?;
    (1..=MAX_AUTH_TOKEN_TTL_SECONDS)
        .contains(&seconds)
        .then_some(seconds)
}

pub(crate) fn env_bool(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_bool_flag(&value))
}

pub(crate) fn parse_bool_flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub(crate) fn env_requests_secure_cookie() -> bool {
    ["XRAYC_ENV", "APP_ENV", "RUST_ENV"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .any(|value| value.eq_ignore_ascii_case("production"))
}

pub(crate) fn default_plan_multiplier() -> f64 {
    1.0
}

pub(crate) fn default_currency() -> String {
    "USDT".to_string()
}

pub(crate) fn default_duration_days() -> i32 {
    30
}

pub(crate) fn default_plan_sort_weight() -> i32 {
    100
}

pub(crate) fn default_sales_landing_json() -> serde_json::Value {
    serde_json::json!({
        "hero": {
            "eyebrow": "XRAYC ACCESS",
            "title": "高速 VPN 节点",
            "subtitle": "面向 AI、游戏和跨境办公的稳定中转服务，订阅只展示中转入口，出口资源由后台统一调度。",
            "primary_cta_text": "查看套餐",
            "primary_cta_href": "/plans",
            "secondary_cta_text": "登录账户",
            "secondary_cta_href": "/login"
        },
        "stats": [
            {"value": "V2", "label": "中转接入控制"},
            {"value": "1", "label": "统一套餐额度"},
            {"value": "24h", "label": "自动订阅更新"}
        ],
        "features": [
            {"title": "订阅入口稳定", "description": "客户端只连接中转入口，后台可替换出口池和第三方上游。"},
            {"title": "统一流量计费", "description": "所有中转入口共享套餐流量，套餐倍率可追溯。"},
            {"title": "运营中心观测", "description": "中转节点主动上报在线、连接、速率和探测状态。"}
        ],
        "scenarios": ["AI 工具访问", "游戏加速", "跨境办公", "流媒体订阅"],
        "faq": [
            {"question": "订阅会暴露出口地址吗？", "answer": "不会。订阅只包含中转入口，第三方出口和真实上游凭据只下发给 access-agent。"},
            {"question": "支持哪些客户端？", "answer": "当前订阅输出 Clash/mihomo YAML，适合 Clash Verge Rev 和 mihomo 类客户端。"}
        ],
        "footer": "XrayC V2"
    })
}

pub(crate) fn default_third_party() -> String {
    "third_party".to_string()
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn empty_object() -> serde_json::Value {
    serde_json::json!({})
}

pub(crate) fn default_redeem_count() -> i32 {
    1
}

pub(crate) fn default_usdt() -> String {
    "USDT".to_string()
}
