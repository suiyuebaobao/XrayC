/// 数据库测试分片 71。
// 本文件覆盖 backup_config 设置的默认值、规范化、脱敏与写时保留。
// 敏感项（SSH 密码 / 邮件附件口令）一律明文落库、输出脱敏成 _set 布尔。
// 纯函数用例不访问数据库；PostgreSQL 用例缺 DATABASE_URL 时脱敏跳过。
// 父级 tests 模块提供公共导入、数据库锁和 helper（pg_test_guard 等）。
// 断言只使用示例地址与占位凭据，不写真实主机 / 口令 / token。
// 备份策略与旧 access_operations.database_backup 解耦，这里只验证新键。
// 后续新增 backup_config 纯函数用例可优先补在本分片，避免旧分片膨胀。
// 注释使用中文，方便后续子任务继续分工。
// 本头部满足前十行中文注释约束。

#[test]
fn test_default_backup_config_shape() {
    // 默认结构必须字段齐全、类型正确，与 spec §4 一致。
    let d = crate::default_backup_config();

    let o = &d["offsite"];
    // offsite 改为「共享异地服务器配置」：不再有 enabled/schedule（不再是独立备份模式）。
    assert!(o.get("enabled").is_none(), "offsite 不应再有 enabled");
    assert!(o.get("schedule").is_none(), "offsite 不应再有 schedule");
    assert_eq!(o["ssh_host"], "");
    assert_eq!(o["ssh_port"], 22);
    assert_eq!(o["ssh_user"], "root");
    assert_eq!(o["ssh_password"], "");
    assert_eq!(o["remote_dir"], "/var/backups/xrayc");
    assert_eq!(o["pubkey_installed"], false);
    assert_eq!(o["pubkey_fingerprint"], "");
    assert_eq!(o["retention_days"], 30);

    let f = &d["full"];
    assert_eq!(f["enabled"], true);
    assert_eq!(f["exclude_traffic_logs"], false);
    assert_eq!(f["schedule"]["kind"], "interval");
    assert_eq!(f["schedule"]["interval_minutes"], 1440);
    assert_eq!(f["schedule"]["cron"], "");
    assert_eq!(f["retention_days"], 30);
    // 全量默认存储位置：仅本机。
    assert_eq!(f["destination"], "local");

    let w = &d["wal"];
    assert_eq!(w["enabled"], false);
    assert_eq!(w["configured"], false);
    assert_eq!(w["full_backup_schedule"]["kind"], "interval");
    assert_eq!(w["full_backup_schedule"]["interval_minutes"], 10080);
    assert_eq!(w["full_backup_schedule"]["cron"], "");
    assert_eq!(w["retention_full"], 4);
    // WAL 默认存储位置：仅本机。
    assert_eq!(w["destination"], "local");

    let e = &d["email"];
    assert_eq!(e["enabled"], false);
    assert!(e["recipients"].is_array());
    assert_eq!(e["recipients"].as_array().unwrap().len(), 0);
    assert_eq!(e["attach_encrypted"], true);
    assert_eq!(e["attach_passphrase"], "");
    assert_eq!(e["max_attach_mb"], 20);
    // 默认超上限处理：仅通知。
    assert_eq!(e["oversize_mode"], "notify");
    assert_eq!(e["notify_on_success"], true);
    assert_eq!(e["notify_on_failure"], true);
}

#[test]
fn test_normalize_backup_config_destination_and_oversize_mode() {
    // destination 非法回 local，oversize_mode 非法回 notify；合法值原样保留。
    let n = crate::normalize_backup_config(json!({
        "full": {"destination": "weird"},
        "wal": {"destination": "offsite"},
        "email": {"oversize_mode": "nope"}
    }));
    assert_eq!(n["full"]["destination"], "local", "非法 destination 回 local");
    assert_eq!(n["wal"]["destination"], "offsite", "合法 destination 保留");
    assert_eq!(
        n["email"]["oversize_mode"], "notify",
        "非法 oversize_mode 回 notify"
    );

    // 三档 destination + split 均为合法值，原样保留。
    let n2 = crate::normalize_backup_config(json!({
        "full": {"destination": "both"},
        "wal": {"destination": "local"},
        "email": {"oversize_mode": "split"}
    }));
    assert_eq!(n2["full"]["destination"], "both");
    assert_eq!(n2["wal"]["destination"], "local");
    assert_eq!(n2["email"]["oversize_mode"], "split");
}

#[test]
fn test_normalize_backup_config_strips_legacy_offsite_mode_fields() {
    // 旧库残留的 offsite.enabled/schedule 在规范化时被剥离（offsite 不再是独立模式）。
    let n = crate::normalize_backup_config(json!({
        "offsite": {
            "enabled": true,
            "schedule": {"kind": "after_full"},
            "ssh_host": "192.0.2.30"
        }
    }));
    assert!(
        n["offsite"].get("enabled").is_none(),
        "enabled 应被剥离: {}",
        n["offsite"]
    );
    assert!(
        n["offsite"].get("schedule").is_none(),
        "schedule 应被剥离: {}",
        n["offsite"]
    );
    // 连接字段与保留天数仍保留。
    assert_eq!(n["offsite"]["ssh_host"], "192.0.2.30");
    assert_eq!(n["offsite"]["retention_days"], 30);
}

#[test]
fn test_normalize_backup_config_clamps_out_of_range() {
    // 越界字段被钳制到合法边界；非法调度类型回默认 interval。
    let n = crate::normalize_backup_config(json!({
        "offsite": {"ssh_port": 70000, "retention_days": 99999},
        "full": {"schedule": {"kind": "weird", "interval_minutes": 0}, "retention_days": 0},
        "wal": {"full_backup_schedule": {"kind": "cron", "interval_minutes": -5}, "retention_full": 0},
        "email": {"max_attach_mb": 999}
    }));

    assert_eq!(n["offsite"]["ssh_port"], 65535);
    assert_eq!(n["offsite"]["retention_days"], 3650);
    assert_eq!(n["full"]["retention_days"], 1);
    // 非法 kind → 回默认 interval。
    assert_eq!(n["full"]["schedule"]["kind"], "interval");
    // interval_minutes 下限 1。
    assert_eq!(n["full"]["schedule"]["interval_minutes"], 1);
    // 合法 kind=cron 保留，interval_minutes 仍钳到下限。
    assert_eq!(n["wal"]["full_backup_schedule"]["kind"], "cron");
    assert_eq!(n["wal"]["full_backup_schedule"]["interval_minutes"], 1);
    assert_eq!(n["wal"]["retention_full"], 1);
    assert_eq!(n["email"]["max_attach_mb"], 50);
}

#[test]
fn test_normalize_backup_config_fills_defaults_and_accepts_after_full() {
    // 空对象：缺失字段用默认补齐（forward-compat）。
    let n = crate::normalize_backup_config(json!({}));
    assert_eq!(n["offsite"]["ssh_port"], 22);
    assert_eq!(n["full"]["schedule"]["kind"], "interval");
    assert_eq!(n["email"]["max_attach_mb"], 20);
    assert_eq!(n["wal"]["retention_full"], 4);

    // after_full 是合法调度类型；下限字段钳到 1。
    let n2 = crate::normalize_backup_config(json!({
        "full": {"schedule": {"kind": "after_full"}},
        "offsite": {"ssh_port": 0},
        "email": {"max_attach_mb": 0}
    }));
    assert_eq!(n2["full"]["schedule"]["kind"], "after_full");
    assert_eq!(n2["offsite"]["ssh_port"], 1);
    assert_eq!(n2["email"]["max_attach_mb"], 1);
}

#[test]
fn test_public_backup_config_redacts_secrets() {
    // 有凭据 → _set:true 且无明文；非敏感字段保留。
    let with = crate::public_backup_config(json!({
        "offsite": {"ssh_password": "SECRET_PW_VALUE", "ssh_host": "192.0.2.1"},
        "email": {"attach_passphrase": "SECRET_PASS_VALUE", "enabled": true}
    }));
    assert_eq!(with["offsite"]["ssh_password_set"], json!(true));
    assert!(with["offsite"].get("ssh_password").is_none());
    assert_eq!(with["offsite"]["ssh_host"], "192.0.2.1");
    assert_eq!(with["email"]["attach_passphrase_set"], json!(true));
    assert!(with["email"].get("attach_passphrase").is_none());
    assert_eq!(with["email"]["enabled"], true);

    // 明文绝不出现在序列化输出里。
    let serialized = serde_json::to_string(&with).unwrap();
    assert!(!serialized.contains("SECRET_PW_VALUE"));
    assert!(!serialized.contains("SECRET_PASS_VALUE"));

    // 默认（空凭据）→ _set:false，无明文字段。
    let empty = crate::public_backup_config(crate::default_backup_config());
    assert_eq!(empty["offsite"]["ssh_password_set"], json!(false));
    assert!(empty["offsite"].get("ssh_password").is_none());
    assert_eq!(empty["email"]["attach_passphrase_set"], json!(false));
    assert!(empty["email"].get("attach_passphrase").is_none());
}

#[test]
fn test_preserve_write_only_backup_config() {
    let current = json!({
        "offsite": {"ssh_password": "OLD_PW"},
        "email": {"attach_passphrase": "OLD_PASS"}
    });

    // 空新值 → 保留旧凭据。
    let out = crate::store::json_util::preserve_write_only_backup_config(
        json!({"offsite": {"ssh_password": ""}, "email": {"attach_passphrase": ""}}),
        &current,
    );
    assert_eq!(out["offsite"]["ssh_password"], json!("OLD_PW"));
    assert_eq!(out["email"]["attach_passphrase"], json!("OLD_PASS"));

    // 占位符 *** → 等同空，保留旧凭据。
    let out = crate::store::json_util::preserve_write_only_backup_config(
        json!({"offsite": {"ssh_password": "***"}, "email": {"attach_passphrase": "***"}}),
        &current,
    );
    assert_eq!(out["offsite"]["ssh_password"], json!("OLD_PW"));
    assert_eq!(out["email"]["attach_passphrase"], json!("OLD_PASS"));

    // 真实新值 → 覆盖旧凭据。
    let out = crate::store::json_util::preserve_write_only_backup_config(
        json!({"offsite": {"ssh_password": "NEW_PW"}, "email": {"attach_passphrase": "NEW_PASS"}}),
        &current,
    );
    assert_eq!(out["offsite"]["ssh_password"], json!("NEW_PW"));
    assert_eq!(out["email"]["attach_passphrase"], json!("NEW_PASS"));
}

#[tokio::test]
async fn test_pg_backup_config_roundtrip_and_write_only() {
    // backup_config 读写闭环 + 写时保留（需要真实 PostgreSQL）：
    // 写凭据 → 读回原值；占位符/空更新 → 旧凭据保留；真实新值 → 更新；公开输出脱敏。
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping PostgreSQL backup config roundtrip test");
        return;
    };
    let _guard = pg_test_guard().await;
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate("../../migrations").await.unwrap();

    // 首次写入：配异地连接 + ssh_password + email 口令 + 自定义调度与端口 + 存储位置。
    let updated = store
        .update_backup_config_json(json!({
            "offsite": {"ssh_host": "192.0.2.10", "ssh_password": "PW1", "ssh_port": 2222},
            "full": {"schedule": {"kind": "interval", "interval_minutes": 720}, "destination": "both"},
            "email": {"enabled": true, "attach_passphrase": "PASS1", "oversize_mode": "split"}
        }))
        .await
        .unwrap();
    // offsite 不再有 enabled（已被规范化剥离）。
    assert!(updated["offsite"].get("enabled").is_none());
    assert_eq!(updated["offsite"]["ssh_password"], json!("PW1"));
    assert_eq!(updated["offsite"]["ssh_port"], 2222);
    assert_eq!(updated["full"]["schedule"]["interval_minutes"], 720);
    assert_eq!(updated["full"]["destination"], "both");
    assert_eq!(updated["email"]["oversize_mode"], "split");
    assert_eq!(updated["email"]["attach_passphrase"], json!("PASS1"));

    // 读回：凭据保持。
    let read_back = store.backup_config_json().await.unwrap();
    assert_eq!(read_back["offsite"]["ssh_password"], json!("PW1"));
    assert_eq!(read_back["email"]["attach_passphrase"], json!("PASS1"));

    // 占位符 / 空更新 → 写时保留旧凭据。
    store
        .update_backup_config_json(json!({
            "offsite": {"ssh_password": "***"},
            "email": {"attach_passphrase": ""}
        }))
        .await
        .unwrap();
    let after = store.backup_config_json().await.unwrap();
    assert_eq!(after["offsite"]["ssh_password"], json!("PW1"));
    assert_eq!(after["email"]["attach_passphrase"], json!("PASS1"));

    // 真实新密码 → 更新。
    store
        .update_backup_config_json(json!({"offsite": {"ssh_password": "PW2"}}))
        .await
        .unwrap();
    let after2 = store.backup_config_json().await.unwrap();
    assert_eq!(after2["offsite"]["ssh_password"], json!("PW2"));

    // 公开脱敏：凭据出 _set:true 且无明文。
    let public = crate::public_backup_config(after2);
    assert_eq!(public["offsite"]["ssh_password_set"], json!(true));
    assert!(public["offsite"].get("ssh_password").is_none());
    assert_eq!(public["email"]["attach_passphrase_set"], json!(true));
    assert!(public["email"].get("attach_passphrase").is_none());
}
