//! 本模块维护站点设置相关的默认值和 JSON 归一化。
//! 函数只处理 serde_json::Value，不访问数据库。
//! PgStore 仍负责读取、写入和合并 site_settings 表。
//! 认证安全、订阅配置、销售页和运维策略在这里分组。
//! 运维探测策略输出复用 crate root 的轻量结构体。
//! 写入前归一化保持历史字段名和默认值兼容。
//! 本模块不处理字段加密和 SMTP 密码保留逻辑。
//! 新增站点设置默认值应优先放在本模块。
//! 本模块不得包含 SQL、PgStore 方法或事务流程。
//! 文件行数保持低于 500 行，便于后续维护。

use serde_json::{json, Value};

use super::{merge_json, AccessProbePolicy, DatabaseBackupPolicy, TrafficLogRetentionPolicy};

pub(super) fn default_auth_security_setting() -> Value {
    json!({
        "require_invite_code": false,
        "allow_user_invite_generation": false,
        "max_invite_codes_per_user": 0,
        "captcha": {
            "register_enabled": false,
            "user_login_enabled": false,
            "admin_login_enabled": false,
            "ttl_seconds": 60
        },
        "email_verification": {
            "enabled": false,
            "allowed_domains": [],
            "cooldown_seconds": 60,
            "smtp_host": "",
            "smtp_port": 587,
            "smtp_username": "",
            "smtp_from": "",
            "smtp_password": ""
        },
        "login_guard": {
            "enabled": true,
            "failure_threshold": 5,
            "lock_minutes": 5
        }
    })
}

pub(super) fn default_subscription_setting() -> Value {
    let rules = default_subscription_rules();
    json!({
        "profile_name": "XrayC",
        "mixed_port": 7890,
        "allow_lan": false,
        "mode": "rule",
        "log_level": "info",
        "auto_test_enabled": false,
        "auto_test_name": "自动选择",
        "auto_test_url": "http://cp.cloudflare.com/generate_204",
        "auto_test_interval_seconds": 86400,
        "block_unhealthy_lines": false,
        "default_rules": rules.clone(),
        "rules": rules
    })
}

fn default_subscription_rules() -> Vec<&'static str> {
    vec![
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
        "DOMAIN-SUFFIX,gtimg.com,DIRECT",
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
        "MATCH,PROXY",
    ]
}

pub(super) fn normalize_subscription_setting(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        let mode = object
            .get("mode")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("rule")
            .to_ascii_lowercase();
        object.insert(
            "mode".to_string(),
            json!(if mode == "global" { "global" } else { "rule" }),
        );

        object.remove("proxy_groups");
        object.remove("proxyGroups");
        object
            .entry("block_unhealthy_lines".to_string())
            .or_insert_with(|| json!(false));
    }
    value
}

pub(super) fn default_sales_landing_setting() -> Value {
    json!({
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
            {
                "title": "订阅入口稳定",
                "description": "客户端只连接中转入口，后台可替换出口池和第三方上游。"
            },
            {
                "title": "统一流量计费",
                "description": "所有中转入口共享套餐流量，套餐倍率可追溯。"
            },
            {
                "title": "运营中心观测",
                "description": "中转节点主动上报在线、连接、速率和探测状态。"
            }
        ],
        "scenarios": [
            "AI 工具访问",
            "游戏加速",
            "跨境办公",
            "流媒体订阅"
        ],
        "faq": [
            {
                "question": "订阅会暴露出口地址吗？",
                "answer": "不会。订阅只包含中转入口，第三方出口和真实上游凭据只下发给 access-agent。"
            },
            {
                "question": "支持哪些客户端？",
                "answer": "当前订阅输出 Clash/mihomo YAML，适合 Clash Verge Rev 和 mihomo 类客户端。"
            }
        ],
        "footer": "XrayC V2"
    })
}

pub(super) fn default_access_operations_setting() -> Value {
    json!({
        "probe_policy": {
            "exit_auto_failover_enabled": true,
            "exit_failure_threshold": 3,
            "exit_recovery_threshold": 2,
            "exit_window_minutes": 10,
            "exit_probe_interval_seconds": 300,
            "probe_queue_batch_size": 100,
            "max_pending_probe_tasks": 5,
            "max_probe_task_delivery_count": 3
        },
        "traffic_log_retention": {
            "detail_retention_days": 14,
            "prune_enabled": true,
            "delete_batch_size": 5000
        },
        "database_backup": {
            "enabled": true,
            "interval_days": 1,
            "retention_days": 30
        }
    })
}

pub(super) fn normalize_access_operations_setting(value: Value) -> Value {
    let mut merged = merge_json(default_access_operations_setting(), value);
    let policy = access_probe_policy_from_setting(&merged);
    let retention = traffic_log_retention_policy_from_setting(&merged);
    let backup = database_backup_policy_from_setting(&merged);
    if let Some(object) = merged.as_object_mut() {
        let probe_policy = object.entry("probe_policy").or_insert_with(|| json!({}));
        if let Some(probe_object) = probe_policy.as_object_mut() {
            probe_object.insert(
                "exit_auto_failover_enabled".to_string(),
                json!(policy.exit_auto_failover_enabled),
            );
            probe_object.insert(
                "exit_failure_threshold".to_string(),
                json!(policy.exit_failure_threshold),
            );
            probe_object.insert(
                "exit_recovery_threshold".to_string(),
                json!(policy.exit_recovery_threshold),
            );
            probe_object.insert(
                "exit_window_minutes".to_string(),
                json!(policy.exit_window_minutes),
            );
            probe_object.insert(
                "exit_probe_interval_seconds".to_string(),
                json!(policy.exit_probe_interval_seconds),
            );
            probe_object.insert(
                "probe_queue_batch_size".to_string(),
                json!(policy.probe_queue_batch_size),
            );
            probe_object.insert(
                "max_pending_probe_tasks".to_string(),
                json!(policy.max_pending_probe_tasks),
            );
            probe_object.insert(
                "max_probe_task_delivery_count".to_string(),
                json!(policy.max_probe_task_delivery_count),
            );
        }
        let retention_policy = object
            .entry("traffic_log_retention")
            .or_insert_with(|| json!({}));
        if let Some(retention_object) = retention_policy.as_object_mut() {
            retention_object.insert(
                "detail_retention_days".to_string(),
                json!(retention.detail_retention_days),
            );
            retention_object.insert("prune_enabled".to_string(), json!(retention.prune_enabled));
            retention_object.insert(
                "delete_batch_size".to_string(),
                json!(retention.delete_batch_size),
            );
        }
        let backup_policy = object.entry("database_backup").or_insert_with(|| json!({}));
        if let Some(backup_object) = backup_policy.as_object_mut() {
            backup_object.insert("enabled".to_string(), json!(backup.enabled));
            backup_object.insert("interval_days".to_string(), json!(backup.interval_days));
            backup_object.insert("retention_days".to_string(), json!(backup.retention_days));
        }
    }
    merged
}

pub(super) fn access_probe_policy_from_setting(value: &Value) -> AccessProbePolicy {
    let probe = value.get("probe_policy").unwrap_or(value);
    AccessProbePolicy {
        exit_auto_failover_enabled: json_bool(
            probe,
            &[
                "exit_auto_failover_enabled",
                "auto_failover_enabled",
                "enabled",
            ],
            true,
        ),
        exit_failure_threshold: json_i64(
            probe,
            &["exit_failure_threshold", "failure_threshold"],
            3,
            1,
            20,
        ),
        exit_recovery_threshold: json_i64(
            probe,
            &["exit_recovery_threshold", "recovery_threshold"],
            2,
            1,
            20,
        ),
        exit_window_minutes: json_i64(
            probe,
            &["exit_window_minutes", "window_minutes"],
            10,
            1,
            120,
        ),
        exit_probe_interval_seconds: json_i64(
            probe,
            &["exit_probe_interval_seconds", "probe_interval_seconds"],
            300,
            30,
            86_400,
        ),
        probe_queue_batch_size: json_i64(
            probe,
            &["probe_queue_batch_size", "queue_batch_size"],
            100,
            1,
            1_000,
        ),
        max_pending_probe_tasks: json_i64(
            probe,
            &["max_pending_probe_tasks", "max_pending_tasks"],
            5,
            1,
            100,
        ),
        max_probe_task_delivery_count: json_i64(
            probe,
            &[
                "max_probe_task_delivery_count",
                "max_task_delivery_count",
                "max_delivery_count",
            ],
            3,
            1,
            20,
        ),
    }
}

pub(super) fn traffic_log_retention_policy_from_setting(
    value: &Value,
) -> TrafficLogRetentionPolicy {
    let retention = value.get("traffic_log_retention").unwrap_or(value);
    TrafficLogRetentionPolicy {
        detail_retention_days: json_i64(
            retention,
            &["detail_retention_days", "retention_days"],
            14,
            1,
            3650,
        ),
        prune_enabled: json_bool(retention, &["prune_enabled", "enabled"], true),
        delete_batch_size: json_i64(
            retention,
            &["delete_batch_size", "batch_size"],
            5000,
            100,
            100_000,
        ),
    }
}

pub(super) fn database_backup_policy_from_setting(value: &Value) -> DatabaseBackupPolicy {
    let backup = value.get("database_backup").unwrap_or(value);
    DatabaseBackupPolicy {
        enabled: json_bool(backup, &["enabled", "auto_backup_enabled"], true),
        interval_days: json_i64(backup, &["interval_days", "backup_interval_days"], 1, 1, 30),
        retention_days: json_i64(
            backup,
            &["retention_days", "backup_retention_days"],
            30,
            1,
            3650,
        ),
    }
}

fn json_bool(value: &Value, fields: &[&str], default: bool) -> bool {
    fields
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_bool))
        .unwrap_or(default)
}

fn json_i64(value: &Value, fields: &[&str], default: i64, min: i64, max: i64) -> i64 {
    fields
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_i64))
        .unwrap_or(default)
        .clamp(min, max)
}

pub(super) fn public_auth_security(value: Value) -> Value {
    let captcha = value.get("captcha").cloned().unwrap_or_else(|| json!({}));
    let email = value
        .get("email_verification")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let guard = value
        .get("login_guard")
        .cloned()
        .unwrap_or_else(|| json!({}));
    json!({
        "require_invite_code": value.get("require_invite_code").and_then(Value::as_bool).unwrap_or(false),
        "invite_required": value.get("require_invite_code").and_then(Value::as_bool).unwrap_or(false),
        "allow_user_invite_generation": value.get("allow_user_invite_generation").and_then(Value::as_bool).unwrap_or(false),
        "max_invite_codes_per_user": value.get("max_invite_codes_per_user").and_then(Value::as_i64).unwrap_or(0),
        "captcha": captcha,
        "captcha_enabled": captcha.get("register_enabled").and_then(Value::as_bool).unwrap_or(false)
            || captcha.get("user_login_enabled").and_then(Value::as_bool).unwrap_or(false)
            || captcha.get("admin_login_enabled").and_then(Value::as_bool).unwrap_or(false),
        "email_verification": {
            "enabled": email.get("enabled").and_then(Value::as_bool).unwrap_or(false),
            "allowed_domains": email.get("allowed_domains").cloned().unwrap_or_else(|| json!([])),
            "cooldown_seconds": email.get("cooldown_seconds").and_then(Value::as_i64).unwrap_or(60)
        },
        "email_verification_enabled": email.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        "login_guard": guard,
        "login_lock_enabled": guard.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        "login_failure_threshold": guard.get("failure_threshold").and_then(Value::as_i64).unwrap_or(5),
        "login_lock_minutes": guard.get("lock_minutes").and_then(Value::as_i64).unwrap_or(5)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_operations_defaults_include_daily_database_backup() {
        let settings = normalize_access_operations_setting(json!({}));
        let backup = database_backup_policy_from_setting(&settings);

        assert!(backup.enabled);
        assert_eq!(backup.interval_days, 1);
        assert_eq!(backup.retention_days, 30);
    }

    #[test]
    fn access_operations_database_backup_policy_is_clamped() {
        let settings = normalize_access_operations_setting(json!({
            "database_backup": {
                "enabled": false,
                "interval_days": 999,
                "retention_days": 0
            }
        }));
        let backup = database_backup_policy_from_setting(&settings);

        assert!(!backup.enabled);
        assert_eq!(backup.interval_days, 30);
        assert_eq!(backup.retention_days, 1);
    }
}
