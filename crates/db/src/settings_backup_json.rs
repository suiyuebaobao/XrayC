//! 数据库备份配置 backup_config 的默认值、规范化与脱敏定义。
//! 函数只处理 serde_json::Value，不访问数据库或网络。
//! PgStore 负责读取、写入与合并 site_settings 里的 backup_config 项。
//! 敏感项（SSH 密码 / 邮件附件口令）明文落库，输出脱敏成布尔 <field>_set。
//! 从 settings_json.rs 拆出，控制单文件长度并隔离备份域逻辑。
//! 本模块不处理字段加密（已下线），与 SMTP 密码/支付密钥同一明文范式。
//! 写时保留由 store/json_util 的 preserve_write_only_backup_config 承担。
//! 新增备份配置默认值与钳制规则应优先放在本模块。
//! 本模块不得包含 SQL、PgStore 方法或事务流程。
//! 本头部满足前十行中文注释约束。

use serde_json::{json, Map, Value};

use super::merge_json;
use super::store::json_util::secret_value_is_present;

/// backup_config 的默认结构（spec §4）：全量 / WAL / 邮件三种备份方式各带独立开关 + 存储位置，
/// 加一段「共享异地服务器配置」offsite（只存连接与保留天数，不再是独立备份模式，故无 enabled/schedule）。
/// 全量/WAL 各带 `destination`(local/offsite/both) 决定本机保留与是否推异地；邮件带 `oversize_mode`
/// (notify/split) 决定超附件上限时仅通知还是分片多封发送。
/// 敏感字段 ssh_password、attach_passphrase 默认空串，落库明文、输出脱敏。
pub(super) fn default_backup_config() -> Value {
    json!({
        "offsite": {
            "ssh_host": "",
            "ssh_port": 22,
            "ssh_user": "root",
            "ssh_password": "",
            "remote_dir": "/var/backups/xrayc",
            "pubkey_installed": false,
            "pubkey_fingerprint": "",
            "retention_days": 30
        },
        "full": {
            "enabled": true,
            "exclude_traffic_logs": false,
            "schedule": {"kind": "interval", "interval_minutes": 1440, "cron": ""},
            "retention_days": 30,
            "destination": "local"
        },
        "wal": {
            "enabled": false,
            "configured": false,
            "full_backup_schedule": {"kind": "interval", "interval_minutes": 10080, "cron": ""},
            "retention_full": 4,
            "destination": "local"
        },
        "email": {
            "enabled": false,
            "recipients": [],
            "attach_encrypted": true,
            "attach_passphrase": "",
            "max_attach_mb": 20,
            "oversize_mode": "notify",
            "notify_on_success": true,
            "notify_on_failure": true,
            "schedule": {"kind": "interval", "interval_minutes": 1440, "cron": ""}
        }
    })
}

/// 规范化 backup_config：先用默认补齐缺失字段（forward-compat），
/// 再钳制数值字段到合法范围、把非法调度类型回退默认；敏感字段原样保留（不在此处脱敏）。
pub(super) fn normalize_backup_config(value: Value) -> Value {
    let mut merged = merge_json(default_backup_config(), value);
    if let Some(offsite) = merged.get_mut("offsite").and_then(Value::as_object_mut) {
        clamp_i64(offsite, "ssh_port", 22, 1, 65_535);
        clamp_i64(offsite, "retention_days", 30, 1, 3_650);
        // offsite 不再是独立备份模式（改为「共享异地服务器配置」），旧的 enabled/schedule
        // 字段一律剥离，避免残留死字段误导前端或被 worker 误读为触发开关。
        offsite.remove("enabled");
        offsite.remove("schedule");
    }
    if let Some(full) = merged.get_mut("full").and_then(Value::as_object_mut) {
        clamp_i64(full, "retention_days", 30, 1, 3_650);
        // 存储位置枚举：local(仅本机)/offsite(仅异地,推成功删本机)/both(两处都留);非法回 local。
        normalize_enum(full, "destination", &DESTINATION_VALUES, "local");
        if let Some(schedule) = full.get_mut("schedule").and_then(Value::as_object_mut) {
            normalize_schedule(schedule);
        }
    }
    if let Some(wal) = merged.get_mut("wal").and_then(Value::as_object_mut) {
        // retention_full 只有下限 1（保留几个 base backup），无业务上限。
        clamp_i64(wal, "retention_full", 4, 1, i64::MAX);
        normalize_enum(wal, "destination", &DESTINATION_VALUES, "local");
        if let Some(schedule) = wal
            .get_mut("full_backup_schedule")
            .and_then(Value::as_object_mut)
        {
            normalize_schedule(schedule);
        }
    }
    if let Some(email) = merged.get_mut("email").and_then(Value::as_object_mut) {
        clamp_i64(email, "max_attach_mb", 20, 1, 50);
        // 超附件上限处理方式：notify(仅通知)/split(分片多封发送);非法回 notify。
        normalize_enum(email, "oversize_mode", &OVERSIZE_MODE_VALUES, "notify");
        // 邮件是完全独立的备份方式，有自己的调度（间隔/cron），不跟随全量；归一化其 schedule。
        if let Some(schedule) = email.get_mut("schedule").and_then(Value::as_object_mut) {
            normalize_schedule(schedule);
        }
    }
    merged
}

/// 全量/WAL 存储位置合法取值（destination 枚举白名单）。
const DESTINATION_VALUES: [&str; 3] = ["local", "offsite", "both"];
/// 邮件超上限处理方式合法取值（oversize_mode 枚举白名单）。
const OVERSIZE_MODE_VALUES: [&str; 2] = ["notify", "split"];

/// 把对象里的枚举字符串字段校验到白名单：缺失/非白名单值一律回退默认值（防前端传非法档）。
fn normalize_enum(object: &mut Map<String, Value>, key: &str, allowed: &[&str], default: &str) {
    let normalized = object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| allowed.contains(value))
        .unwrap_or(default)
        .to_string();
    object.insert(key.to_string(), json!(normalized));
}

/// 把对象里的整数字段钳制到 [min,max]；字段缺失或非整数（非法）时回默认值再钳制。
fn clamp_i64(object: &mut Map<String, Value>, key: &str, default: i64, min: i64, max: i64) {
    let raw = object.get(key).and_then(Value::as_i64).unwrap_or(default);
    object.insert(key.to_string(), json!(raw.clamp(min, max)));
}

/// 规范化单个调度对象：kind 只允许 interval/cron/after_full，非法回默认 interval；
/// interval_minutes 下限 1（tick 支持分钟级），无业务上限。
fn normalize_schedule(schedule: &mut Map<String, Value>) {
    let kind = match schedule.get("kind").and_then(Value::as_str) {
        Some("cron") => "cron",
        Some("after_full") => "after_full",
        _ => "interval",
    };
    schedule.insert("kind".to_string(), json!(kind));
    clamp_i64(schedule, "interval_minutes", 1_440, 1, i64::MAX);
}

/// backup_config 的两条 write-only 凭据路径（脱敏输出复用同一组路径）。
const BACKUP_WRITE_ONLY_PATHS: [&[&str]; 2] = [
    &["offsite", "ssh_password"],
    &["email", "attach_passphrase"],
];

/// 把 backup_config 脱敏成可公开返回的形态：两个 write-only 凭据字段从对象里移除，
/// 改成同名 `<field>_set` 布尔（凭据非空 → true，空 → false），明文绝不出现在输出。
pub(super) fn public_backup_config(mut value: Value) -> Value {
    for path in BACKUP_WRITE_ONLY_PATHS {
        let Some((field, parent_path)) = path.split_last() else {
            continue;
        };
        // 用 JSON pointer 定位父对象，缺失则跳过该路径。
        let pointer = format!("/{}", parent_path.join("/"));
        let Some(parent_object) = value.pointer_mut(&pointer).and_then(Value::as_object_mut) else {
            continue;
        };
        let is_set = parent_object
            .get(*field)
            .map(secret_value_is_present)
            .unwrap_or(false);
        parent_object.remove(*field);
        parent_object.insert(format!("{field}_set"), json!(is_set));
    }
    value
}
