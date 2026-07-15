//! 支付配置的默认值、脱敏与 write-only 路径定义。
//! 函数只处理 serde_json::Value，不访问数据库或网络。
//! PgStore 负责读取、写入与合并 site_settings 表里的 payment 项。
//! 公开输出把四个敏感凭据字段脱敏成布尔 <field>_set，不回显明文。
//! 写时保留共用本模块的 PAYMENT_WRITE_ONLY_PATHS，避免两边路径漂移。
//! 从 settings_json.rs 拆出，控制单文件长度并隔离支付域逻辑。
//! 本模块不处理字段加密，敏感凭据以明文落库（与现有 SMTP 范式一致）。
//! 新增支付 provider 默认值与脱敏路径应优先放在本模块。
//! 本模块不得包含 SQL、PgStore 方法或事务流程。
//! 本头部满足前十行中文注释约束。

use serde_json::{json, Value};

use super::store::json_util::secret_value_is_present;

pub(super) fn default_payment_setting() -> Value {
    json!({
        "enabled": false,
        "default_channel": "",
        "providers": {
            "alipay": {
                "enabled": false,
                "environment": "sandbox",
                "app_id": "",
                "app_private_key": "",
                "alipay_public_key": "",
                "notify_url": ""
            },
            "epay": {
                "enabled": false,
                "api_url": "",
                "pid": "",
                "key": "",
                "notify_url": ""
            },
            "wechat": {
                "enabled": false,
                "mch_id": "",
                "app_id": "",
                "api_v3_key": "",
                "cert_serial_no": "",
                "private_key": "",
                "notify_url": ""
            }
        }
    })
}

/// 支付配置的四个 write-only 凭据字段路径（与 store 写时保留共用同一组路径）。
/// 公开输出和写时保留都按这组路径处理敏感凭据，避免两边不一致。
pub(super) const PAYMENT_WRITE_ONLY_PATHS: [&[&str]; 4] = [
    &["providers", "alipay", "app_private_key"],
    &["providers", "epay", "key"],
    &["providers", "wechat", "api_v3_key"],
    &["providers", "wechat", "private_key"],
];

/// 把支付配置脱敏成可公开返回的形态：四个 write-only 凭据字段从对象里移除，
/// 改成同名 `<field>_set` 布尔（凭据非空 → true，空 → false），不回显明文。
pub(super) fn public_payment_settings(mut value: Value) -> Value {
    for path in PAYMENT_WRITE_ONLY_PATHS {
        let Some((field, parent_path)) = path.split_last() else {
            continue;
        };
        // 用 JSON pointer 定位到字段所在的父对象，缺失则跳过该路径。
        let pointer = format!("/{}", parent_path.join("/"));
        let Some(parent_object) = value.pointer_mut(&pointer).and_then(Value::as_object_mut) else {
            continue;
        };
        // 凭据是否已设置：移除明文后用同名 _set 布尔表达。
        let is_set = parent_object
            .get(*field)
            .map(secret_value_is_present)
            .unwrap_or(false);
        parent_object.remove(*field);
        parent_object.insert(format!("{field}_set"), json!(is_set));
    }
    value
}
