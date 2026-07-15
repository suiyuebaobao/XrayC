//! 邮件发文件模块:附件加密(openssl)+ 经共享 SMTP 发送 + 超限降级(仅通知 / 分片多封)+ 通知。
//! 备份成功后按 `backup_config.email` 取最新 dump,按需 openssl 加密作附件,经 `xrayc_backup::send_email`
//! 发给收件人;附件超 `max_attach_mb` 时按 `email.oversize_mode` 分叉:notify=仅通知(不静默)、
//! split=先(按需)整份加密 → `split` 切片 → 每片一封邮件(主题标进度、正文附中文重组说明)。
//! send_backup_email/send_notification 已由编排层(经 run.rs 的 run_email)在邮件方式触发时调用;
//! 失败转发失败通知。安全边界:临时加密/分片文件权限 0600、用后整目录必删;错误/日志绝不含口令。
//! 纯函数(附件决策/命名/主题/重组正文/split 参数)拆到 `format` 子模块单测;本文件只收敛 IO 与编排。
//! 本头部满足前十行中文说明约束。
// EmailSendReport.decision 等字段供状态/排障保留但当前仅内部与测试读,豁免 dead_code。
#![allow(dead_code)]

mod format;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde_json::Value;
use xrayc_backup::{EmailAttachment, SmtpConfig};

use format::{
    attachment_filename, backup_email_body, decide_attachment, encryption_mode, notification_body,
    notification_subject, openssl_encrypt_args, oversize_mode, oversize_notice_body, recipients_of,
    redact_passphrase, should_notify, split_args, split_email_subject, split_part_name,
    split_reassembly_body, AttachDecision, EncryptionMode, OversizeMode,
};

const BACKUP_SUBJECT: &str = "XrayC 数据库备份";

/// 一次备份邮件发送的结果快照,供编排层回写状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmailSendReport {
    /// 因无收件人在入口被跳过(未发任何邮件)。
    pub(crate) skipped: bool,
    /// 实际采用的附件决策(skipped 时为 None)。
    pub(crate) decision: Option<AttachDecision>,
    /// 是否带附件发送(NotifyOnly / skipped 为 false)。
    pub(crate) attached: bool,
    /// 附件是否加密。
    pub(crate) encrypted: bool,
    /// 附件文件名(带附件时有值)。
    pub(crate) attachment_name: Option<String>,
    /// 实际发送的附件字节数(NotifyOnly / skipped 为 0)。
    pub(crate) attachment_bytes: u64,
    /// 分片路数(仅 Split 分支 >0):超上限走分片时的分片总数,供状态展示。
    pub(crate) split_parts: u64,
}

/// 读取文件字节数(用于判附件决策)。
fn file_len(path: &Path) -> anyhow::Result<u64> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("读取备份文件大小失败: {}", path.display()))?;
    Ok(meta.len())
}

/// 生成唯一的临时密文路径(进程号 + 纳秒,避免并发碰撞)。
fn temp_encrypted_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "xrayc-backup-enc-{}-{}.enc",
        std::process::id(),
        nanos
    ))
}

/// 以 0600 预建密文文件:openssl 以 O_TRUNC 覆写会保留该权限位,防止密文一度可读。
fn create_secure_file(path: &Path) -> anyhow::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("创建临时密文文件失败: {}", path.display()))?;
    Ok(())
}

/// 用 openssl 把 `source` 加密到 `enc_path`(不读回内容,供分片流程对大文件加密到磁盘再切片)。
/// 失败摘要过 `redact_passphrase` 脱敏,绝不打印命令行 args(内含 `pass:<口令>`)。
async fn run_openssl_encrypt(
    passphrase: &str,
    source: &Path,
    enc_path: &Path,
) -> anyhow::Result<()> {
    create_secure_file(enc_path)?;
    let args = openssl_encrypt_args(passphrase, source, enc_path);
    let output = tokio::process::Command::new("openssl")
        .args(&args)
        .kill_on_drop(true)
        .output()
        .await
        // context 只描述动作,绝不带 args/口令。
        .context("执行 openssl 加密失败")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "openssl 加密失败: {}",
            redact_passphrase(&stderr, passphrase)
        );
    }
    Ok(())
}

/// 用 openssl 把 `source` 加密到 `enc_path` 并读回密文字节(直接附件路径复用)。
async fn encrypt_and_read(
    passphrase: &str,
    source: &Path,
    enc_path: &Path,
) -> anyhow::Result<Vec<u8>> {
    run_openssl_encrypt(passphrase, source, enc_path).await?;
    std::fs::read(enc_path).with_context(|| format!("读取密文文件失败: {}", enc_path.display()))
}

/// 按加密模式取出附件字节:明文直接读;加密则临时加密→读回→立即删临时密文;
/// 要求加密却无口令时拒绝(绝不退回发送明文附件)。返回 (字节, 是否已加密)。
async fn read_attachment_bytes(email: &Value, source: &Path) -> anyhow::Result<(Vec<u8>, bool)> {
    match encryption_mode(email) {
        EncryptionMode::Plain => {
            let bytes = std::fs::read(source)
                .with_context(|| format!("读取备份文件失败: {}", source.display()))?;
            Ok((bytes, false))
        }
        EncryptionMode::MissingPassphrase => {
            bail!("附件加密已开启但未配置口令,拒绝发送未加密附件")
        }
        EncryptionMode::Encrypt(passphrase) => {
            let enc_path = temp_encrypted_path();
            let result = encrypt_and_read(&passphrase, source, &enc_path).await;
            // 无论成败都删临时密文(用后必删)。
            let _ = std::fs::remove_file(&enc_path);
            Ok((result?, true))
        }
    }
}

/// 发送备份邮件:按 `decide_attachment` 选文件,按需加密后作附件经共享 SMTP 发出;
/// 超上限时按 `oversize_mode` 走「仅通知」或「分片多封」(都不静默)。无收件人时入口跳过。
/// 说明:总闸(email.enabled)与「何时调用」由编排层决定,本函数只保证给定 config 下正确工作。
pub(crate) async fn send_backup_email(
    email: &Value,
    smtp: &SmtpConfig,
    dump_path: &Path,
    smaller_dump_path: Option<&Path>,
) -> anyhow::Result<EmailSendReport> {
    let recipients = recipients_of(email);
    if recipients.is_empty() {
        return Ok(EmailSendReport {
            skipped: true,
            decision: None,
            attached: false,
            encrypted: false,
            attachment_name: None,
            attachment_bytes: 0,
            split_parts: 0,
        });
    }
    let max_attach_mb = email
        .get("max_attach_mb")
        .and_then(Value::as_i64)
        .unwrap_or(20);

    let dump_bytes = file_len(dump_path)?;
    let smaller_bytes = match smaller_dump_path {
        Some(p) => Some(file_len(p)?),
        None => None,
    };
    let decision = decide_attachment(dump_bytes, smaller_bytes, max_attach_mb);

    if decision == AttachDecision::NotifyOnly {
        // 超上限:按 oversize_mode 分叉——split 走分片多封,notify 走仅通知(现状,不静默)。
        if oversize_mode(email) == OversizeMode::Split {
            return send_split_backup(email, smtp, dump_path, max_attach_mb, &recipients).await;
        }
        let name = attachment_filename(dump_path, false);
        let body = oversize_notice_body(&name, dump_bytes);
        xrayc_backup::send_email(smtp, &recipients, BACKUP_SUBJECT, &body, None).await?;
        return Ok(EmailSendReport {
            skipped: false,
            decision: Some(AttachDecision::NotifyOnly),
            attached: false,
            encrypted: false,
            attachment_name: None,
            attachment_bytes: 0,
            split_parts: 0,
        });
    }

    // Attach / AttachSmaller:选定源文件(精简版仅 AttachSmaller 用)。
    let source: &Path = if decision == AttachDecision::AttachSmaller {
        smaller_dump_path.expect("AttachSmaller 必有精简 dump 路径")
    } else {
        dump_path
    };
    let (content, encrypted) = read_attachment_bytes(email, source).await?;
    let content_len = content.len() as u64;
    let filename = attachment_filename(source, encrypted);
    let attachment = EmailAttachment {
        filename: filename.clone(),
        content,
        content_type: "application/octet-stream".to_string(),
    };
    let body = backup_email_body(&filename, content_len, encrypted);
    xrayc_backup::send_email(smtp, &recipients, BACKUP_SUBJECT, &body, Some(attachment)).await?;
    Ok(EmailSendReport {
        skipped: false,
        decision: Some(decision),
        attached: true,
        encrypted,
        attachment_name: Some(filename),
        attachment_bytes: content_len,
        split_parts: 0,
    })
}

/// 生成唯一的分片临时目录(进程号 + 纳秒,避免并发碰撞);用后由调用方整目录删除。
fn temp_split_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "xrayc-backup-split-{}-{}",
        std::process::id(),
        nanos
    ))
}

/// 超上限且 oversize_mode=split:先(按需)openssl 加密整份 → `split` 切片 → 每片一封邮件发送。
/// 真实 split/发送在真机执行;分片命名/主题/重组正文等纯逻辑已单测。全程用后清临时目录。
/// 加密开启但缺口令 → 拒绝(绝不发未加密分片);任一步失败上抛给编排层脱敏落状态(不静默)。
async fn send_split_backup(
    email: &Value,
    smtp: &SmtpConfig,
    dump_path: &Path,
    max_attach_mb: i64,
    recipients: &[String],
) -> anyhow::Result<EmailSendReport> {
    let work_dir = temp_split_dir();
    std::fs::create_dir_all(&work_dir)
        .with_context(|| format!("创建分片临时目录失败: {}", work_dir.display()))?;
    // 无论成败都整目录清理(临时明文/密文/分片用后必删)。
    let result =
        split_backup_inner(email, smtp, dump_path, max_attach_mb, recipients, &work_dir).await;
    let _ = std::fs::remove_dir_all(&work_dir);
    result
}

/// 分片发送内层实现(在给定临时目录里加密+切片+逐片发送),错误上抛,清理由外层统一负责。
async fn split_backup_inner(
    email: &Value,
    smtp: &SmtpConfig,
    dump_path: &Path,
    max_attach_mb: i64,
    recipients: &[String],
    work_dir: &Path,
) -> anyhow::Result<EmailSendReport> {
    // 1. 加密决策:要求加密却无口令则拒绝;加密则整份先加密到临时密文,再对密文切片。
    let decrypted_name = attachment_filename(dump_path, false);
    let (source_path, encrypted) = match encryption_mode(email) {
        EncryptionMode::MissingPassphrase => {
            bail!("附件加密已开启但未配置口令,拒绝发送未加密分片")
        }
        EncryptionMode::Plain => (dump_path.to_path_buf(), false),
        EncryptionMode::Encrypt(passphrase) => {
            let enc_path = work_dir.join(attachment_filename(dump_path, true));
            run_openssl_encrypt(&passphrase, dump_path, &enc_path).await?;
            (enc_path, true)
        }
    };
    let merged_name = attachment_filename(&source_path, false);
    let source_bytes = file_len(&source_path)?;

    // 2. split 切片:分片落临时目录,前缀 = <merged_name>.,分片即 <merged_name>.000/001/...
    let out_prefix = work_dir.join(format!("{merged_name}."));
    let args = split_args(max_attach_mb, &source_path, &out_prefix);
    let output = tokio::process::Command::new("split")
        .args(&args)
        .kill_on_drop(true)
        .output()
        .await
        .context("执行 split 分片失败")?;
    if !output.status.success() {
        bail!(
            "split 分片失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    // 3. 收集分片(前缀带点,排除源文件本身),按字典序 = 分片顺序。
    let part_prefix = format!("{merged_name}.");
    let mut parts = collect_split_parts(work_dir, &part_prefix)?;
    parts.sort();
    if parts.is_empty() {
        bail!("split 未产出任何分片文件");
    }
    let total = parts.len();
    let part_glob = format!("{merged_name}.*");
    let body = split_reassembly_body(&merged_name, &decrypted_name, total, &part_glob, encrypted);

    // 4. 每片一封邮件:主题标进度,正文附中文重组说明,附件为该分片。
    for (index0, part_path) in parts.iter().enumerate() {
        let part_name = part_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| split_part_name(&part_prefix, index0));
        let content = std::fs::read(part_path)
            .with_context(|| format!("读取分片失败: {}", part_path.display()))?;
        let attachment = EmailAttachment {
            filename: part_name,
            content,
            content_type: "application/octet-stream".to_string(),
        };
        let subject = split_email_subject(index0 + 1, total);
        xrayc_backup::send_email(smtp, recipients, &subject, &body, Some(attachment)).await?;
    }

    Ok(EmailSendReport {
        skipped: false,
        decision: Some(AttachDecision::Split),
        attached: true,
        encrypted,
        attachment_name: Some(merged_name),
        attachment_bytes: source_bytes,
        split_parts: total as u64,
    })
}

/// 收集临时目录里以 `part_prefix` 打头的分片文件路径(只认普通文件,排除源密文/明文本身)。
fn collect_split_parts(work_dir: &Path, part_prefix: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut parts = Vec::new();
    for entry in std::fs::read_dir(work_dir)
        .with_context(|| format!("读取分片目录失败: {}", work_dir.display()))?
    {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(part_prefix) {
            continue;
        }
        if entry.metadata().map(|m| m.is_file()).unwrap_or(false) {
            parts.push(entry.path());
        }
    }
    Ok(parts)
}

/// 发送成功/失败通知(纯文本)。notify_on_success/failure + 是否有收件人门控;
/// detail 由编排侧构造并已脱敏(成功含时间/大小/文件名,失败含脱敏错误摘要)。
pub(crate) async fn send_notification(
    email: &Value,
    smtp: &SmtpConfig,
    success: bool,
    detail: &str,
) -> anyhow::Result<()> {
    if !should_notify(email, success) {
        return Ok(());
    }
    let recipients = recipients_of(email);
    let subject = notification_subject(success);
    let body = notification_body(success, detail);
    xrayc_backup::send_email(smtp, &recipients, &subject, &body, None).await
}
