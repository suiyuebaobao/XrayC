//! 本模块封装 Xray 统计接口和统计用户邮箱规则。
//! 统计邮箱用于把核心程序上报数据映射回访问线路和用户。
//! 统计接口入站与路由规则由主编译入口按开关注入。

use serde_json::{json, Value};

pub const XRAY_STATS_API_TAG: &str = "xrayc-api";
pub const XRAY_STATS_API_LISTEN: &str = "127.0.0.1";
pub const XRAY_STATS_API_PORT: u16 = 10085;

const STATS_EMAIL_PREFIX: &str = "xrayc-line-";
const LEGACY_STATS_EMAIL_PREFIX: &str = "rp-line-";
const STATS_EMAIL_SEPARATOR: &str = "--";

pub fn stats_user_email(line_id: &str, xray_user_key: &str) -> String {
    format!("{STATS_EMAIL_PREFIX}{line_id}{STATS_EMAIL_SEPARATOR}{xray_user_key}")
}

pub fn parse_stats_user_email(email: &str) -> Option<(String, String)> {
    let rest = email
        .strip_prefix(STATS_EMAIL_PREFIX)
        .or_else(|| email.strip_prefix(LEGACY_STATS_EMAIL_PREFIX))?;
    let (line_id, xray_user_key) = rest.split_once(STATS_EMAIL_SEPARATOR)?;
    if line_id.trim().is_empty() || xray_user_key.trim().is_empty() {
        return None;
    }
    Some((line_id.to_owned(), xray_user_key.to_owned()))
}

pub(crate) fn compile_stats_api_inbound() -> Value {
    json!({
        "tag": XRAY_STATS_API_TAG,
        "listen": XRAY_STATS_API_LISTEN,
        "port": XRAY_STATS_API_PORT,
        "protocol": "dokodemo-door",
        "settings": {
            "address": XRAY_STATS_API_LISTEN,
        },
    })
}

pub(crate) fn compile_stats_api_routing_rule() -> Value {
    json!({
        "type": "field",
        "inboundTag": [XRAY_STATS_API_TAG],
        "outboundTag": XRAY_STATS_API_TAG,
    })
}
