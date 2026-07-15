//! 通用 JSON 合并与写时保留 helper（与字段加密无关）。
//! 本文件承载 PgStore 共享的纯 JSON 工具函数。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 这里只做明文 JSON 处理，敏感字段一律直读直写。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use serde_json::Value;

/// 判断字符串型敏感值是否真实存在（非空白）。
/// 仅用于 SMTP 密码这类“写时保留旧值”的判断，不做任何加密。
pub(crate) fn secret_value_is_present(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.trim().is_empty(),
        _ => false,
    }
}

/// 处理 SMTP 密码这类只写不回显字段：
/// 前端传 `***`/空且要求不清空时，保留库里现有明文密码，避免被占位符覆盖。
/// 显式 `clear_smtp_password=true` 时清空密码。该逻辑与加密无关，仅做明文取舍。
pub(crate) fn preserve_write_only_smtp_password(mut value: Value, current: &Value) -> Value {
    if let Some(email) = value
        .get_mut("email_verification")
        .and_then(Value::as_object_mut)
    {
        let clear_password = email
            .remove("clear_smtp_password")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if clear_password {
            email.insert("smtp_password".to_string(), Value::String(String::new()));
            return value;
        }
        // SMTP 历史行为把占位符 `***` 等同于空值（要求保留旧密码）；
        // 先把占位符归一成空字符串，再交给通用 write-only 保留逻辑统一处理。
        if email
            .get("smtp_password")
            .and_then(Value::as_str)
            .is_some_and(|password| password == "***")
        {
            email.insert("smtp_password".to_string(), Value::String(String::new()));
        }
    }

    preserve_write_only_at(value, current, &[&["email_verification", "smtp_password"]])
}

/// 处理 backup_config 里的 SSH 密码 / 邮件附件口令这类只写不回显字段：
/// 前端传占位符 `***` 表示"保留旧值"，先把 `***` 归一成空串，再交给通用
/// preserve_write_only_at 对两条路径做写时保留（新值空/缺 → 用旧值，非空 → 覆盖）。
/// 与字段加密无关，仅做明文取舍，与 SMTP/支付写时保留同一范式。
pub(crate) fn preserve_write_only_backup_config(mut value: Value, current: &Value) -> Value {
    for path in [["offsite", "ssh_password"], ["email", "attach_passphrase"]] {
        if let Some(parent) = value.get_mut(path[0]).and_then(Value::as_object_mut) {
            if parent.get(path[1]).and_then(Value::as_str) == Some("***") {
                parent.insert(path[1].to_string(), Value::String(String::new()));
            }
        }
    }
    preserve_write_only_at(
        value,
        current,
        &[
            &["offsite", "ssh_password"],
            &["email", "attach_passphrase"],
        ],
    )
}

/// 通用 write-only 字段保留：对每条路径，若新值缺失或为空（非真实存在），
/// 则用 current 在同路径的值写回 value；新值非空则保留新值（空值不清掉旧凭据）。
/// 逐层定位/创建对象，与字段加密无关，仅做明文取舍，被 SMTP/支付等设置复用。
pub(crate) fn preserve_write_only_at(
    mut value: Value,
    current: &Value,
    paths: &[&[&str]],
) -> Value {
    for path in paths {
        // 新值该路径是否已是真实存在的非空凭据；是则保留新值，不做兜底。
        let new_present = path_value(&value, path)
            .map(secret_value_is_present)
            .unwrap_or(false);
        if new_present {
            continue;
        }
        // 取 current 同路径旧值；旧值不存在或本身为空时无需兜底。
        let Some(current_value) = path_value(current, path) else {
            continue;
        };
        if !secret_value_is_present(current_value) {
            continue;
        }
        // 逐层 get_mut/创建对象，把旧凭据写回新值对应路径。
        set_path_value(&mut value, path, current_value.clone());
    }
    value
}

/// 按字符串路径只读定位 JSON 子值，路径任一层缺失返回 None。
fn path_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

/// 按字符串路径写入 JSON 子值，沿途缺失或非对象的层会被创建/覆盖为对象。
fn set_path_value(value: &mut Value, path: &[&str], new_value: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = value;
    for key in parents {
        if !current.is_object() {
            *current = Value::Object(serde_json::Map::new());
        }
        current = current
            .as_object_mut()
            .expect("已确保是对象")
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
    }
    if !current.is_object() {
        *current = Value::Object(serde_json::Map::new());
    }
    current
        .as_object_mut()
        .expect("已确保是对象")
        .insert((*last).to_string(), new_value);
}

/// 递归合并两个 JSON 文档：对象按 key 深合并，其它类型直接用 patch 覆盖 base。
/// 这是通用 JSON 工具，与字段加密无关，被 settings/auth 等多处复用。
pub(crate) fn merge_json(mut base: Value, patch: Value) -> Value {
    match (&mut base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, value) in patch {
                let next = if let Some(existing) = base.remove(&key) {
                    merge_json(existing, value)
                } else {
                    value
                };
                base.insert(key, next);
            }
            Value::Object(base.clone())
        }
        (_, patch) => patch,
    }
}
