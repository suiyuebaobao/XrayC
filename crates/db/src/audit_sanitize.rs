//! 本模块负责审计摘要的纯脱敏处理。
//! 输入和输出均为 JSON 值，不访问数据库。
//! 存储前和展示前共享同一套敏感字段规则。
//! 已脱敏标记可在存储路径中保留，避免重复丢失上下文。
//! 展示路径会重新收敛敏感信息，防止旁路读取泄漏。
//! 规则覆盖密码、令牌、订阅链接、代理链接和密钥文本。
//! 字符串会限制最大长度，防止异常请求撑大审计日志。
//! 数组和对象递归深度都有上限，避免恶意嵌套。
//! 新增规则应保持确定性，便于审计和测试复现。
//! 本模块不得包含业务写入逻辑。

use serde_json::{json, Map, Value};

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn public_audit_summary(value: Value) -> Value {
    audit_summary_with_mode(value, false)
}

pub(crate) fn stored_audit_summary(value: Value) -> Value {
    audit_summary_with_mode(value, true)
}

fn audit_summary_with_mode(value: Value, preserve_redacted_markers: bool) -> Value {
    let (sanitized, redacted) = sanitize_audit_summary_value(value, 0, preserve_redacted_markers);
    if !redacted {
        return sanitized;
    }

    match sanitized {
        Value::Object(mut map) => {
            map.insert("sensitive_fields_redacted".to_string(), Value::Bool(true));
            Value::Object(map)
        }
        Value::Null => json!({"sensitive_fields_redacted": true}),
        value => json!({
            "details": value,
            "sensitive_fields_redacted": true
        }),
    }
}

fn sanitize_audit_summary_value(
    value: Value,
    depth: usize,
    preserve_redacted_markers: bool,
) -> (Value, bool) {
    if depth >= 6 {
        return (Value::String("[truncated]".to_string()), true);
    }

    match value {
        Value::Object(map) => {
            let mut sanitized = Map::new();
            let mut redacted = false;
            for (key, value) in map {
                if preserve_redacted_markers
                    && is_redacted_marker_key(&key)
                    && matches!(value, Value::Bool(_))
                {
                    sanitized.insert(key, value);
                    continue;
                }
                if is_sensitive_audit_summary_key(&key) {
                    redacted = true;
                    continue;
                }
                let (value, value_redacted) =
                    sanitize_audit_summary_value(value, depth + 1, preserve_redacted_markers);
                redacted |= value_redacted;
                sanitized.insert(key, value);
            }
            (Value::Object(sanitized), redacted)
        }
        Value::Array(items) => {
            let mut redacted = false;
            let sanitized = items
                .into_iter()
                .take(50)
                .map(|value| {
                    let (value, value_redacted) =
                        sanitize_audit_summary_value(value, depth + 1, preserve_redacted_markers);
                    redacted |= value_redacted;
                    value
                })
                .collect::<Vec<_>>();
            (Value::Array(sanitized), redacted)
        }
        Value::String(text) if is_sensitive_audit_summary_text(&text) => {
            (Value::String("[redacted]".to_string()), true)
        }
        Value::String(text) => (Value::String(truncate_audit_summary_text(&text)), false),
        value => (value, false),
    }
}

fn is_redacted_marker_key(key: &str) -> bool {
    key.replace('-', "_")
        .to_ascii_lowercase()
        .ends_with("_redacted")
}

fn is_sensitive_audit_summary_key(key: &str) -> bool {
    let key = key.replace('-', "_").to_ascii_lowercase();
    key.ends_with("_redacted")
        || [
            "password",
            "passwd",
            "passphrase",
            "token",
            "secret",
            "private_key",
            "identity_file",
            "proxy_url",
            "subscription_url",
            "subscription_link",
            "subscribe_url",
            "subscribe_link",
            "outbound_config",
            "stream_config",
            "probe_config",
            "inbound_config",
            "raw_payload",
            "ssh_host",
        ]
        .iter()
        .any(|part| key.contains(part))
}

fn is_sensitive_audit_summary_text(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    text.starts_with("sub-")
        || text.contains("/sub/")
        || text.contains("-----begin ")
        || text.contains("password")
        || text.contains("secret")
        || text.contains("token")
        || [
            "socks://",
            "socks5://",
            "http://",
            "https://",
            "vless://",
            "trojan://",
            "ss://",
            "hysteria://",
        ]
        .iter()
        .any(|prefix| text.starts_with(prefix))
}

fn truncate_audit_summary_text(text: &str) -> String {
    const MAX_CHARS: usize = 512;
    let mut chars = text.chars();
    let truncated = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}
