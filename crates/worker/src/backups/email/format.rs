//! 邮件模块的纯函数层:附件决策 / 超上限处理方式 / 加密模式解析 / 附件命名 / 各类正文与主题 /
//! openssl 加密参数 / split 分片参数与命名 / 口令脱敏。全为无 IO 纯逻辑,便于单测覆盖边界,
//! 与真实发送/加密/切片(在 mod.rs 的 IO 层)解耦。
//! 分片相关(split_args/split_part_name/split_email_subject/split_reassembly_body)只产出命令参数与
//! 展示文本,真实 `split`/发送留真机;主题/命名/重组说明纯逻辑在此单测。
//! 敏感项:openssl 口令只进 `-pass pass:<...>`、脱敏用 redact_passphrase,绝不裸露口令。
//! 本文件不含任何 process/fs/网络调用,IO 全部收敛在父模块 mod.rs。
//! 供父模块 mod.rs 以 `pub(super)`/`pub(crate)` 复用。
//! 本头部满足前十行中文注释约束。

use std::path::Path;

use serde_json::Value;

/// 附件决策:直接附全量 / 附精简版 / 仅通知 / 分片多封(都超上限时不静默)。
/// `Split` 不由 `decide_attachment` 产出(它只判 NotifyOnly),而是在超上限且 `oversize_mode=split`
/// 时由发送流程改走分片路径后回填,便于状态与测试区分「仅通知」与「已分片」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttachDecision {
    Attach,
    AttachSmaller,
    NotifyOnly,
    Split,
}

/// 超附件上限处理方式:仅通知(不发文件)/ 分片多封发送。缺失/非法默认 Notify。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OversizeMode {
    Notify,
    Split,
}

/// 解析 email.oversize_mode(纯函数):"split" → Split,其余(含缺失/非法)→ Notify。
pub(super) fn oversize_mode(email: &Value) -> OversizeMode {
    match email.get("oversize_mode").and_then(Value::as_str) {
        Some("split") => OversizeMode::Split,
        _ => OversizeMode::Notify,
    }
}

/// 附件加密模式:不加密 / 加密(带口令)/ 要求加密但缺口令(拒绝发明文)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EncryptionMode {
    Plain,
    Encrypt(String),
    MissingPassphrase,
}

/// 组装 `openssl enc` 加密参数(纯函数,便于单测,口令只进 `-pass` 不裸露)。
pub(super) fn openssl_encrypt_args(
    passphrase: &str,
    in_path: &Path,
    out_path: &Path,
) -> Vec<String> {
    // 对应命令:openssl enc -aes-256-cbc -pbkdf2 -salt -pass pass:<口令> -in <in> -out <out>
    vec![
        "enc".to_string(),
        "-aes-256-cbc".to_string(),
        "-pbkdf2".to_string(),
        "-salt".to_string(),
        "-pass".to_string(),
        format!("pass:{passphrase}"),
        "-in".to_string(),
        in_path.to_string_lossy().into_owned(),
        "-out".to_string(),
        out_path.to_string_lossy().into_owned(),
    ]
}

/// 按 dump 大小与上限决定附件策略(纯函数)。
/// dump≤上限→Attach;超限但有更小精简 dump 且≤上限→AttachSmaller;否则→NotifyOnly(不静默)。
pub(super) fn decide_attachment(
    dump_bytes: u64,
    smaller_bytes: Option<u64>,
    max_attach_mb: i64,
) -> AttachDecision {
    let limit = max_attach_mb.max(0) as u64 * 1024 * 1024;
    if dump_bytes <= limit {
        AttachDecision::Attach
    } else if smaller_bytes.is_some_and(|s| s <= limit) {
        AttachDecision::AttachSmaller
    } else {
        AttachDecision::NotifyOnly
    }
}

/// 解析收件人列表(去空白项,纯函数)。
pub(super) fn recipients_of(email: &Value) -> Vec<String> {
    email
        .get("recipients")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 通知门控:按 notify_on_success/failure(默认开)+ 是否有收件人判定是否发通知(纯函数)。
pub(super) fn should_notify(email: &Value, success: bool) -> bool {
    if recipients_of(email).is_empty() {
        return false;
    }
    let key = if success {
        "notify_on_success"
    } else {
        "notify_on_failure"
    };
    email.get(key).and_then(Value::as_bool).unwrap_or(true)
}

/// 解析附件加密模式(纯函数,attach_encrypted 默认 true)。
pub(super) fn encryption_mode(email: &Value) -> EncryptionMode {
    let encrypted = email
        .get("attach_encrypted")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !encrypted {
        return EncryptionMode::Plain;
    }
    let passphrase = email
        .get("attach_passphrase")
        .and_then(Value::as_str)
        .unwrap_or("");
    if passphrase.is_empty() {
        EncryptionMode::MissingPassphrase
    } else {
        EncryptionMode::Encrypt(passphrase.to_string())
    }
}

/// 依据是否加密派生附件名(加密加 `.enc` 后缀,纯函数,只取文件名不带路径)。
pub(super) fn attachment_filename(source: &Path, encrypted: bool) -> String {
    let base = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup.dump".to_string());
    if encrypted {
        format!("{base}.enc")
    } else {
        base
    }
}

/// 字节数格式化为便于阅读的 MB(纯函数)。
fn format_size(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / 1_048_576.0)
}

/// 备份邮件正文(含文件名/大小/是否加密,不含敏感信息,纯函数)。
pub(super) fn backup_email_body(file_name: &str, size_bytes: u64, encrypted: bool) -> String {
    let enc_note = if encrypted {
        "附件已用 AES-256-CBC 加密,请用备份口令解密。"
    } else {
        "附件未加密。"
    };
    format!(
        "XrayC 数据库备份已作为附件发送。\n文件名: {file_name}\n大小: {}\n{enc_note}",
        format_size(size_bytes)
    )
}

/// 超限降级仅通知的正文(说明备份成功但过大未附,请从异地取,纯函数)。
pub(super) fn oversize_notice_body(file_name: &str, size_bytes: u64) -> String {
    format!(
        "XrayC 数据库备份成功,但文件超过附件上限,未随邮件附带。\n文件名: {file_name}\n大小: {}\n请从异地备份处取回该备份文件。",
        format_size(size_bytes)
    )
}

/// 分片文件名数字后缀长度(GNU split `--suffix-length`);零填充保证字典序 = 分片顺序。
const SPLIT_SUFFIX_LENGTH: usize = 3;

/// 组装 `split` 分片命令参数(纯函数,便于单测)。
/// 对应命令:`split -b <max>M -d --suffix-length=3 <in> <out_prefix>` → 分片 `<prefix>000/001/...`。
/// `-d` 用数字后缀(0 起、零填充),配合前缀天然按字典序即分片顺序;max 至少 1MB(防 0/负数)。
pub(super) fn split_args(max_attach_mb: i64, in_path: &Path, out_prefix: &Path) -> Vec<String> {
    let size = max_attach_mb.max(1);
    vec![
        "-b".to_string(),
        format!("{size}M"),
        "-d".to_string(),
        format!("--suffix-length={SPLIT_SUFFIX_LENGTH}"),
        in_path.to_string_lossy().into_owned(),
        out_prefix.to_string_lossy().into_owned(),
    ]
}

/// 由「前缀 + 0 起分片序号」推出分片文件名(纯函数),与 `split -d` 的零填充后缀口径一致。
pub(super) fn split_part_name(prefix: &str, index0: usize) -> String {
    format!("{prefix}{index0:0width$}", width = SPLIT_SUFFIX_LENGTH)
}

/// 分片邮件主题(纯函数):人读 1 起的「第 N/M 片」,标清进度防漏收。
pub(super) fn split_email_subject(part_1based: usize, total: usize) -> String {
    format!("XrayC 数据库备份分片 第 {part_1based}/{total} 片")
}

/// 分片邮件正文:中文重组说明(纯函数)。下载全部 → 按序 `cat 分片* > 文件` → (加密则)openssl 解密。
/// `part_glob` 是分片通配(如 `xrayc.dump.enc.*`),零填充后缀保证 `cat` 展开即正确顺序。
pub(super) fn split_reassembly_body(
    merged_name: &str,
    decrypted_name: &str,
    total: usize,
    part_glob: &str,
    encrypted: bool,
) -> String {
    let decrypt_step = if encrypted {
        format!(
            "\n4. 解密还原:openssl enc -d -aes-256-cbc -pbkdf2 -pass pass:<备份口令> -in {merged_name} -out {decrypted_name}"
        )
    } else {
        String::new()
    };
    format!(
        "XrayC 数据库备份文件较大,已分成 {total} 片、分多封邮件发送(本邮件为其中一片)。\n\
         重组步骤:\n\
         1. 下载全部 {total} 片附件到同一目录;\n\
         2. 核对从「第 1 片」到「第 {total} 片」已收齐,缺一不可;\n\
         3. 按顺序合并:cat {part_glob} > {merged_name}{decrypt_step}\n\
         注意:必须收齐所有分片且严格按序号合并,缺片或乱序都会导致文件损坏无法恢复。"
    )
}

/// 通知主题(成功/失败,纯函数)。
pub(super) fn notification_subject(success: bool) -> String {
    if success {
        "XrayC 数据库备份成功".to_string()
    } else {
        "XrayC 数据库备份失败".to_string()
    }
}

/// 通知正文(成功/失败前缀 + 已脱敏的明细,纯函数)。
pub(super) fn notification_body(success: bool, detail: &str) -> String {
    let prefix = if success {
        "数据库备份成功。"
    } else {
        "数据库备份失败。"
    };
    format!("{prefix}\n{detail}")
}

/// 把文本里出现的明文口令替换为掩码(纯函数,口令为空时原样返回,避免误伤整串)。
pub(super) fn redact_passphrase(text: &str, passphrase: &str) -> String {
    if passphrase.is_empty() {
        return text.to_string();
    }
    text.replace(passphrase, "***")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn openssl_args_are_exact_and_passphrase_only_in_pass() {
        let args = openssl_encrypt_args(
            "secret-pass",
            Path::new("/backups/a.dump"),
            Path::new("/tmp/a.enc"),
        );
        assert!(args.contains(&"enc".to_string()));
        assert!(args.contains(&"-aes-256-cbc".to_string()));
        assert!(args.contains(&"-pbkdf2".to_string()));
        assert!(args.contains(&"-salt".to_string()));
        let pass_idx = args.iter().position(|a| a == "-pass").expect("应含 -pass");
        assert_eq!(args[pass_idx + 1], "pass:secret-pass");
        let in_idx = args.iter().position(|a| a == "-in").expect("应含 -in");
        assert_eq!(args[in_idx + 1], "/backups/a.dump");
        let out_idx = args.iter().position(|a| a == "-out").expect("应含 -out");
        assert_eq!(args[out_idx + 1], "/tmp/a.enc");
        // 口令绝不作为独立裸参出现,只在 pass:<...> 里。
        assert!(!args.iter().any(|a| a == "secret-pass"));
    }

    #[test]
    fn decide_attach_when_dump_equals_limit() {
        // 1MB 上限,dump 恰等于上限 → 直接附全量。
        assert_eq!(
            decide_attachment(1_048_576, None, 1),
            AttachDecision::Attach
        );
    }

    #[test]
    fn decide_attach_when_dump_below_limit() {
        assert_eq!(
            decide_attachment(500_000, Some(400_000), 1),
            AttachDecision::Attach
        );
    }

    #[test]
    fn decide_attach_smaller_when_over_but_smaller_fits() {
        assert_eq!(
            decide_attachment(2_000_000, Some(800_000), 1),
            AttachDecision::AttachSmaller
        );
    }

    #[test]
    fn decide_notify_only_when_both_over() {
        assert_eq!(
            decide_attachment(2_000_000, Some(1_500_000), 1),
            AttachDecision::NotifyOnly
        );
    }

    #[test]
    fn decide_notify_only_when_over_and_no_smaller() {
        assert_eq!(
            decide_attachment(2_000_000, None, 1),
            AttachDecision::NotifyOnly
        );
    }

    #[test]
    fn recipients_filters_blank_entries() {
        let email = json!({ "recipients": ["a@x.test", "  ", "b@x.test"] });
        assert_eq!(recipients_of(&email), vec!["a@x.test", "b@x.test"]);
    }

    #[test]
    fn recipients_empty_when_absent_or_empty() {
        assert!(recipients_of(&json!({})).is_empty());
        assert!(recipients_of(&json!({ "recipients": [] })).is_empty());
    }

    #[test]
    fn should_notify_respects_success_and_failure_flags() {
        let email = json!({
            "notify_on_success": true,
            "notify_on_failure": false,
            "recipients": ["a@x.test"]
        });
        assert!(should_notify(&email, true));
        assert!(!should_notify(&email, false));
    }

    #[test]
    fn should_notify_false_without_recipients() {
        let email = json!({ "notify_on_success": true, "recipients": [] });
        assert!(!should_notify(&email, true));
    }

    #[test]
    fn encryption_mode_variants() {
        assert_eq!(
            encryption_mode(&json!({ "attach_encrypted": true, "attach_passphrase": "pp" })),
            EncryptionMode::Encrypt("pp".to_string())
        );
        assert_eq!(
            encryption_mode(&json!({ "attach_encrypted": true, "attach_passphrase": "" })),
            EncryptionMode::MissingPassphrase
        );
        assert_eq!(
            encryption_mode(&json!({ "attach_encrypted": false })),
            EncryptionMode::Plain
        );
    }

    #[test]
    fn attachment_filename_adds_enc_suffix_when_encrypted() {
        let path = Path::new("/backups/xrayc-postgres-2026.dump");
        assert_eq!(attachment_filename(path, false), "xrayc-postgres-2026.dump");
        assert_eq!(
            attachment_filename(path, true),
            "xrayc-postgres-2026.dump.enc"
        );
    }

    #[test]
    fn backup_body_mentions_file_and_encryption() {
        let body = backup_email_body("xrayc-postgres-2026.dump.enc", 5_242_880, true);
        assert!(body.contains("xrayc-postgres-2026.dump.enc"), "应含文件名");
        assert!(body.contains("加密"), "加密附件应提示已加密");
    }

    #[test]
    fn oversize_notice_mentions_offsite_and_file() {
        let body = oversize_notice_body("xrayc-postgres.dump", 30_000_000);
        assert!(body.contains("超"), "应说明超上限");
        assert!(body.contains("异地"), "应提示从异地取");
        assert!(body.contains("xrayc-postgres.dump"), "应含文件名");
    }

    #[test]
    fn notification_body_carries_prefix_and_detail() {
        let ok = notification_body(true, "时间 2026, 大小 5MB, 文件 xrayc.dump");
        assert!(ok.contains("成功"), "成功前缀");
        assert!(ok.contains("xrayc.dump"), "含明细文件名");
        let fail = notification_body(false, "pg_dump exited");
        assert!(fail.contains("失败"), "失败前缀");
        assert!(fail.contains("pg_dump exited"), "含脱敏错误摘要");
    }

    #[test]
    fn notification_subject_differs_by_result() {
        assert_ne!(notification_subject(true), notification_subject(false));
    }

    #[test]
    fn redact_passphrase_masks_and_keeps_when_empty() {
        let safe = redact_passphrase("openssl error near secret-pass here", "secret-pass");
        assert!(!safe.contains("secret-pass"), "脱敏后不得含明文口令");
        assert!(safe.contains("***"), "应以掩码替换");
        assert_eq!(redact_passphrase("hello", ""), "hello");
    }

    #[test]
    fn format_size_renders_mb() {
        assert!(format_size(1_048_576).contains("MB"));
    }

    // ---- 超上限处理方式 / 分片纯逻辑 ----

    #[test]
    fn oversize_mode_parses_split_and_defaults_notify() {
        assert_eq!(
            oversize_mode(&json!({"oversize_mode": "split"})),
            OversizeMode::Split
        );
        assert_eq!(
            oversize_mode(&json!({"oversize_mode": "notify"})),
            OversizeMode::Notify
        );
        // 缺失 / 非法 → 默认 Notify(不误分片)。
        assert_eq!(oversize_mode(&json!({})), OversizeMode::Notify);
        assert_eq!(
            oversize_mode(&json!({"oversize_mode": "bogus"})),
            OversizeMode::Notify
        );
    }

    #[test]
    fn split_args_are_exact_and_size_floors_at_one() {
        // 对应命令:split -b <max>M -d --suffix-length=3 <in> <prefix>
        let args = split_args(
            20,
            Path::new("/tmp/x.dump.enc"),
            Path::new("/tmp/x.dump.enc."),
        );
        assert_eq!(args[0], "-b");
        assert_eq!(args[1], "20M");
        assert_eq!(args[2], "-d");
        assert_eq!(args[3], "--suffix-length=3");
        assert_eq!(args[4], "/tmp/x.dump.enc");
        assert_eq!(args[5], "/tmp/x.dump.enc.");
        // max<1 归一到 1M(防 0/负数导致 split 报错)。
        let floored = split_args(0, Path::new("/a"), Path::new("/b."));
        assert_eq!(floored[1], "1M");
    }

    #[test]
    fn split_part_name_zero_pads_to_suffix_length() {
        // 与 split -d --suffix-length=3 的零填充后缀一致(0 起)。
        assert_eq!(split_part_name("x.dump.enc.", 0), "x.dump.enc.000");
        assert_eq!(split_part_name("x.dump.enc.", 5), "x.dump.enc.005");
        assert_eq!(split_part_name("x.dump.enc.", 42), "x.dump.enc.042");
    }

    #[test]
    fn split_subject_shows_one_based_progress() {
        assert_eq!(split_email_subject(1, 3), "XrayC 数据库备份分片 第 1/3 片");
        assert_eq!(split_email_subject(3, 3), "XrayC 数据库备份分片 第 3/3 片");
    }

    #[test]
    fn split_reassembly_body_has_download_cat_and_decrypt_steps() {
        // 加密分片:正文含下载全部、按序 cat 合并、openssl 解密三要点。
        let body = split_reassembly_body(
            "xrayc-postgres-2026.dump.enc",
            "xrayc-postgres-2026.dump",
            3,
            "xrayc-postgres-2026.dump.enc.*",
            true,
        );
        assert!(body.contains("下载全部"), "应提示下载全部分片");
        assert!(
            body.contains("cat xrayc-postgres-2026.dump.enc.* > xrayc-postgres-2026.dump.enc"),
            "应含按序合并命令: {body}"
        );
        assert!(body.contains("openssl enc -d"), "加密分片应含解密步骤");
        assert!(
            body.contains("第 1 片") && body.contains("第 3 片"),
            "应标清收齐范围"
        );

        // 未加密分片:无解密步骤。
        let plain = split_reassembly_body("a.dump", "a.dump", 2, "a.dump.*", false);
        assert!(!plain.contains("openssl enc -d"), "未加密不应含解密步骤");
        assert!(plain.contains("cat a.dump.* > a.dump"));
    }
}
