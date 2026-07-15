//! 本模块解析订阅生成的可配置选项。
//! 选项只影响客户端可见的配置外形，不允许把订阅切换为直连策略。
//! 字段解析同时兼容蛇形命名和前端使用的驼峰命名。

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionOptions {
    pub profile_name: String,
    pub mixed_port: u16,
    pub allow_lan: bool,
    pub mode: String,
    pub log_level: Option<String>,
    pub update_interval_hours: u32,
    pub auto_test_enabled: bool,
    pub auto_test_name: String,
    pub auto_test_url: String,
    pub auto_test_interval_seconds: u32,
    pub block_unhealthy_lines: bool,
    pub rules: Vec<String>,
}

impl Default for SubscriptionOptions {
    fn default() -> Self {
        Self {
            profile_name: "XrayC".to_string(),
            mixed_port: 7890,
            allow_lan: false,
            mode: "rule".to_string(),
            log_level: Some("info".to_string()),
            update_interval_hours: 24,
            auto_test_enabled: false,
            auto_test_name: "自动选择".to_string(),
            auto_test_url: "http://cp.cloudflare.com/generate_204".to_string(),
            auto_test_interval_seconds: 86400,
            block_unhealthy_lines: false,
            rules: Vec::new(),
        }
    }
}

impl SubscriptionOptions {
    pub fn from_json(value: &Value) -> Self {
        let mut options = Self::default();
        if let Some(profile_name) = string_field(value, &["profile_name", "profileName", "name"]) {
            options.profile_name = profile_name;
        }
        if let Some(mixed_port) = u64_field(value, &["mixed_port", "mixedPort"]) {
            options.mixed_port = mixed_port.min(u16::MAX as u64) as u16;
        }
        if let Some(allow_lan) = bool_field(value, &["allow_lan", "allowLan"]) {
            options.allow_lan = allow_lan;
        }
        if let Some(mode) = string_field(value, &["mode"]) {
            options.mode = normalize_profile_mode(&mode).to_string();
        }
        options.log_level = string_field(value, &["log_level", "logLevel"]).or(options.log_level);
        if let Some(hours) = u64_field(value, &["update_interval_hours", "updateIntervalHours"]) {
            options.update_interval_hours = hours.clamp(1, 168) as u32;
        }
        if let Some(rules) = string_array_field(value, &["default_rules", "defaultRules", "rules"])
        {
            options.rules = rules;
        }
        if let Some(enabled) = bool_field(value, &["auto_test_enabled", "autoTestEnabled"]) {
            options.auto_test_enabled = enabled;
        }
        if let Some(name) = string_field(value, &["auto_test_name", "autoTestName"]) {
            options.auto_test_name = name;
        }
        if let Some(url) = string_field(value, &["auto_test_url", "autoTestUrl"]) {
            options.auto_test_url = url;
        }
        if let Some(interval) = u64_field(
            value,
            &["auto_test_interval_seconds", "autoTestIntervalSeconds"],
        ) {
            options.auto_test_interval_seconds = interval.clamp(1, 86400) as u32;
        }
        if let Some(enabled) = bool_field(value, &["block_unhealthy_lines", "blockUnhealthyLines"])
        {
            options.block_unhealthy_lines = enabled;
        }
        options
    }
}

pub(super) fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn string_array_field(value: &Value, keys: &[&str]) -> Option<Vec<String>> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
}

fn u64_field(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}

fn bool_field(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_bool))
}

fn normalize_profile_mode(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "global" => "global",
        _ => "rule",
    }
}
