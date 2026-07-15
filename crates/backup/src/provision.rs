//! 异地备份机 SSH 免密 provision:生成中心机密钥对、用 SSH 密码装公钥、验证免密、取指纹。
//! 供 api(`test-and-provision` 端点在请求内一次性 SSH 装公钥)与 worker(持私钥卷)共用。
//! 安全边界:SSH 密码只在内存传参给 `sshpass`,绝不写日志;任何命令输出/错误摘要都过
//! `sanitize_provision_output` 脱敏(抹掉密码与 host);host 必须是 IPv4(域名尤其 CF 橙云
//! 无法直连 SSH,拒绝)。命令参数组装抽成纯函数便于单测,不真连服务器即可校验精确性。
//! 私钥文件权限 0600、只落密钥目录;公钥可展示指纹。本头部满足中文说明约束。

use std::path::Path;

use anyhow::{bail, Context, Result};

/// 中心机密钥对文件名(私钥 0600 / 公钥)。
const PRIVATE_KEY_NAME: &str = "id_ed25519";
const PUBLIC_KEY_NAME: &str = "id_ed25519.pub";
/// 生成密钥时写入的注释,便于在异地机 authorized_keys 里辨识来源。
const KEY_COMMENT: &str = "xrayc-backup";

/// provision 结果:是否成功、公钥指纹(成功时有)、脱敏后的可展示消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionResult {
    pub ok: bool,
    pub fingerprint: Option<String>,
    pub message: String,
}

/// 单条 SSH 命令输出的截断上限(与备份脱敏口径一致,防超长噪声)。
const MAX_OUTPUT_CHARS: usize = 240;

/// host 是否为合法 IPv4 字面量(拒域名 / IPv6)。
/// 只接受纯 IPv4 字面量:域名尤其 CF 橙云无法直连 SSH,IPv6 也不在本功能配方内。
fn is_ipv4_host(host: &str) -> bool {
    host.parse::<std::net::Ipv4Addr>().is_ok()
}

/// 组装 `ssh-keygen` 生成密钥的参数(不含二进制名)。
/// 对应配方:`ssh-keygen -t ed25519 -N '' -C xrayc-backup -f <keypath>`。
fn ssh_keygen_args(key_path: &Path) -> Vec<String> {
    vec![
        "-t".to_string(),
        "ed25519".to_string(),
        // 空口令(无人值守免密):`-N ''`。
        "-N".to_string(),
        String::new(),
        "-C".to_string(),
        KEY_COMMENT.to_string(),
        "-f".to_string(),
        key_path.to_string_lossy().into_owned(),
    ]
}

/// 组装 `ssh-keygen -lf <pub>` 取指纹的参数。
fn ssh_keygen_fingerprint_args(pub_path: &Path) -> Vec<String> {
    vec!["-lf".to_string(), pub_path.to_string_lossy().into_owned()]
}

/// 组装装公钥的远端 shell 脚本(去重追加到 authorized_keys)。
/// 对应配方:`mkdir -p ~/.ssh && chmod 700 ~/.ssh &&
///           grep -qF '<pub>' ~/.ssh/authorized_keys 2>/dev/null || echo '<pub>' >> ~/.ssh/authorized_keys`。
fn remote_authorized_keys_script(pubkey: &str) -> String {
    format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && \
grep -qF '{pubkey}' ~/.ssh/authorized_keys 2>/dev/null || \
echo '{pubkey}' >> ~/.ssh/authorized_keys"
    )
}

/// 组装装公钥用的 `ssh` 参数(在 `sshpass -p <pass> ssh` 之后,不含密码)。
/// 强制走密码认证、禁公钥,确保首装能进机器;脚本作最后一个参数由远端 shell 执行。
fn install_pubkey_ssh_args(port: u16, user: &str, host: &str, pubkey: &str) -> Vec<String> {
    vec![
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "PreferredAuthentications=password".to_string(),
        "-o".to_string(),
        "PubkeyAuthentication=no".to_string(),
        "-p".to_string(),
        port.to_string(),
        format!("{user}@{host}"),
        remote_authorized_keys_script(pubkey),
    ]
}

/// 组装密钥免密验证用的 `ssh` 参数(不含二进制名)。
/// 只用密钥认证(`IdentitiesOnly=yes` + `PreferredAuthentications=publickey`),
/// 远端只跑 `echo ok`,确认公钥确已生效。
fn verify_pubkey_ssh_args(port: u16, user: &str, host: &str, key_path: &Path) -> Vec<String> {
    vec![
        "-i".to_string(),
        key_path.to_string_lossy().into_owned(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "IdentitiesOnly=yes".to_string(),
        "-o".to_string(),
        "PreferredAuthentications=publickey".to_string(),
        "-p".to_string(),
        port.to_string(),
        format!("{user}@{host}"),
        "echo ok".to_string(),
    ]
}

/// 从 `ssh-keygen -lf` 输出解析指纹(第二个空白分隔字段,如 `SHA256:xxxx`)。
/// 输出形如 `256 SHA256:abc... xrayc-backup (ED25519)`,取第二段即指纹。
fn parse_fingerprint(raw: &str) -> Option<String> {
    raw.split_whitespace().nth(1).map(str::to_string)
}

/// 脱敏命令输出:抹掉 SSH 密码与 host,压平换行并截断。
/// 密码/host 为空时不替换(避免把整串误抹成掩码);空结果回退通用提示。
fn sanitize_provision_output(raw: &[u8], password: &str, host: &str) -> String {
    let mut text = String::from_utf8_lossy(raw).replace(['\n', '\r'], " ");
    if !password.is_empty() {
        text = text.replace(password, "***");
    }
    if !host.is_empty() {
        text = text.replace(host, "<host>");
    }
    let text = text.trim();
    if text.is_empty() {
        "ssh command failed".to_string()
    } else {
        text.chars().take(MAX_OUTPUT_CHARS).collect()
    }
}

/// 确保备份密钥对存在:无 `id_ed25519` 则 `ssh-keygen` 生成(私钥 0600),已存在读公钥返回。
/// 幂等:已存在时绝不重生成覆盖(私钥保持不变),只读公钥串返回。
pub fn ensure_backup_keypair(key_dir: &Path) -> Result<String> {
    let key_path = key_dir.join(PRIVATE_KEY_NAME);
    let pub_path = key_dir.join(PUBLIC_KEY_NAME);
    if key_path.exists() {
        // 已存在:直接读公钥返回,不触碰私钥。
        let pubkey = std::fs::read_to_string(&pub_path)
            .with_context(|| format!("读取已存在备份公钥失败: {}", pub_path.display()))?;
        return Ok(pubkey.trim().to_string());
    }
    std::fs::create_dir_all(key_dir)
        .with_context(|| format!("创建备份密钥目录失败: {}", key_dir.display()))?;
    let output = std::process::Command::new("ssh-keygen")
        .args(ssh_keygen_args(&key_path))
        .output()
        .context("执行 ssh-keygen 生成备份密钥失败")?;
    if !output.status.success() {
        // 生成失败摘要脱敏(本处无密码/host,仅压平截断,防泄露路径噪声)。
        bail!(
            "ssh-keygen 生成密钥失败: {}",
            sanitize_provision_output(&output.stderr, "", "")
        );
    }
    // ssh-keygen 默认已给私钥 0600;稳妥起见在 unix 上再显式收紧一次。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
    }
    let pubkey = std::fs::read_to_string(&pub_path)
        .with_context(|| format!("读取新生成备份公钥失败: {}", pub_path.display()))?;
    Ok(pubkey.trim().to_string())
}

/// 用密钥取公钥指纹(`ssh-keygen -lf <pub>`);失败返回 None(指纹只用于展示,不阻塞成功)。
async fn read_fingerprint(pub_path: &Path) -> Option<String> {
    let output = tokio::process::Command::new("ssh-keygen")
        .args(ssh_keygen_fingerprint_args(pub_path))
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_fingerprint(&String::from_utf8_lossy(&output.stdout))
}

/// 装公钥并验证免密:校验 IPv4 → 确保密钥 → sshpass 装公钥 → 密钥验证 → 取指纹。
/// 失败一律返回受控 `ProvisionResult{ok:false, message: 脱敏}`(不 panic、不外泄密码/host);
/// 仅二进制缺失等无法启动的异常经 `?` 返回 Err(其 context 固定、不含密码/host)。
pub async fn provision_offsite(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    key_dir: &Path,
) -> Result<ProvisionResult> {
    // 1) host 必须是 IPv4:域名尤其 CF 橙云无法直连 SSH,直接拒。
    if !is_ipv4_host(host) {
        return Ok(ProvisionResult {
            ok: false,
            fingerprint: None,
            message: "SSH 主机必须是 IPv4 地址(域名/CF 橙云无法直连 SSH)".to_string(),
        });
    }
    // 2) 确保中心机密钥对存在,取公钥。
    let pubkey = ensure_backup_keypair(key_dir)?;
    let key_path = key_dir.join(PRIVATE_KEY_NAME);
    let pub_path = key_dir.join(PUBLIC_KEY_NAME);

    // 3) 用页面填的 SSH 密码 sshpass 进异地机装公钥(去重追加)。密码只作 sshpass 独立参数、
    //    不进任何日志;install args 纯函数里本就不含密码。
    let install = tokio::process::Command::new("sshpass")
        .arg("-p")
        .arg(password)
        .arg("ssh")
        .args(install_pubkey_ssh_args(port, user, host, &pubkey))
        .kill_on_drop(true)
        .output()
        .await
        .context("执行 sshpass 装公钥失败")?;
    if !install.status.success() {
        return Ok(ProvisionResult {
            ok: false,
            fingerprint: None,
            message: format!(
                "装公钥失败: {}",
                sanitize_provision_output(&install.stderr, password, host)
            ),
        });
    }

    // 4) 立即用密钥验证免密成功(只走 publickey)。
    let verify = tokio::process::Command::new("ssh")
        .args(verify_pubkey_ssh_args(port, user, host, &key_path))
        .kill_on_drop(true)
        .output()
        .await
        .context("执行 ssh 免密验证失败")?;
    if !verify.status.success() {
        return Ok(ProvisionResult {
            ok: false,
            fingerprint: None,
            message: format!(
                "免密验证失败: {}",
                sanitize_provision_output(&verify.stderr, password, host)
            ),
        });
    }

    // 5) 成功:取指纹(仅展示用,取不到不影响成功判定)。
    let fingerprint = read_fingerprint(&pub_path).await;
    Ok(ProvisionResult {
        ok: true,
        fingerprint,
        message: "异地备份机公钥已安装,免密登录验证通过".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn ipv4_host_accepts_ipv4_rejects_domain_and_ipv6() {
        assert!(is_ipv4_host("203.0.113.9"), "合法 IPv4 应接受");
        assert!(!is_ipv4_host("backup.example.test"), "域名应拒绝");
        assert!(!is_ipv4_host("2001:db8::1"), "IPv6 应拒绝");
        assert!(!is_ipv4_host(""), "空串应拒绝");
        assert!(!is_ipv4_host("203.0.113.9:22"), "带端口应拒绝");
    }

    #[test]
    fn ssh_keygen_args_are_exact() {
        // 对应配方:ssh-keygen -t ed25519 -N '' -C xrayc-backup -f <keypath>
        assert_eq!(
            ssh_keygen_args(Path::new("/keys/id_ed25519")),
            s(&[
                "-t",
                "ed25519",
                "-N",
                "",
                "-C",
                "xrayc-backup",
                "-f",
                "/keys/id_ed25519",
            ])
        );
    }

    #[test]
    fn fingerprint_args_target_pub_file() {
        assert_eq!(
            ssh_keygen_fingerprint_args(Path::new("/keys/id_ed25519.pub")),
            s(&["-lf", "/keys/id_ed25519.pub"])
        );
    }

    #[test]
    fn remote_script_dedupes_and_appends_pubkey() {
        let script = remote_authorized_keys_script("ssh-ed25519 AAAAKEY xrayc-backup");
        assert!(script.contains("mkdir -p ~/.ssh"), "应建 ~/.ssh");
        assert!(script.contains("chmod 700 ~/.ssh"), "应收紧目录权限");
        assert!(
            script.contains("grep -qF 'ssh-ed25519 AAAAKEY xrayc-backup'"),
            "应用 grep -qF 去重判断"
        );
        assert!(
            script.contains(">> ~/.ssh/authorized_keys"),
            "缺失时应追加到 authorized_keys"
        );
    }

    #[test]
    fn install_ssh_args_match_recipe_and_carry_no_password() {
        let args = install_pubkey_ssh_args(2222, "root", "203.0.113.9", "ssh-ed25519 KEY c");
        // 对应配方:ssh -o StrictHostKeyChecking=accept-new -o PreferredAuthentications=password
        //          -o PubkeyAuthentication=no -p <port> <user>@<host> "<script>"
        assert_eq!(args[0], "-o");
        assert_eq!(args[1], "StrictHostKeyChecking=accept-new");
        assert!(args.contains(&"PreferredAuthentications=password".to_string()));
        assert!(args.contains(&"PubkeyAuthentication=no".to_string()));
        let p_idx = args.iter().position(|a| a == "-p").expect("应含 -p");
        assert_eq!(args[p_idx + 1], "2222");
        assert_eq!(args[p_idx + 2], "root@203.0.113.9");
        // 最后一个参数是远端装公钥脚本。
        assert!(args.last().unwrap().contains("authorized_keys"));
    }

    #[test]
    fn verify_ssh_args_use_key_and_publickey_only() {
        let args =
            verify_pubkey_ssh_args(2222, "root", "203.0.113.9", Path::new("/keys/id_ed25519"));
        // 对应配方:ssh -i <keypath> -o StrictHostKeyChecking=accept-new -o IdentitiesOnly=yes
        //          -o PreferredAuthentications=publickey -p <port> <user>@<host> 'echo ok'
        assert_eq!(args[0], "-i");
        assert_eq!(args[1], "/keys/id_ed25519");
        assert!(args.contains(&"IdentitiesOnly=yes".to_string()));
        assert!(args.contains(&"PreferredAuthentications=publickey".to_string()));
        let p_idx = args.iter().position(|a| a == "-p").expect("应含 -p");
        assert_eq!(args[p_idx + 1], "2222");
        assert_eq!(args[p_idx + 2], "root@203.0.113.9");
        assert_eq!(args.last().unwrap(), "echo ok");
    }

    #[test]
    fn parse_fingerprint_extracts_second_field() {
        let raw = "256 SHA256:abc123DEF456 xrayc-backup (ED25519)\n";
        assert_eq!(
            parse_fingerprint(raw),
            Some("SHA256:abc123DEF456".to_string())
        );
        assert_eq!(parse_fingerprint(""), None);
        assert_eq!(parse_fingerprint("   \n"), None);
    }

    #[test]
    fn sanitize_output_masks_password_and_host_and_flattens() {
        let raw = b"line1 secretpass\nhost 203.0.113.9 refused\r\ntail";
        let cleaned = sanitize_provision_output(raw, "secretpass", "203.0.113.9");
        assert!(!cleaned.contains("secretpass"), "脱敏后不得含密码");
        assert!(!cleaned.contains("203.0.113.9"), "脱敏后不得含 host");
        assert!(
            !cleaned.contains('\n') && !cleaned.contains('\r'),
            "应压平换行"
        );
    }

    #[test]
    fn sanitize_output_keeps_text_when_password_empty() {
        // 密码为空时不得把整串抹成掩码。
        let cleaned = sanitize_provision_output(b"connection refused", "", "198.51.100.7");
        assert!(cleaned.contains("connection refused"));
    }

    #[test]
    fn ensure_keypair_reads_existing_without_regenerating() {
        // 预置已存在的密钥对:ensure 应直接读公钥返回、不覆盖私钥(幂等)。
        let dir = std::env::temp_dir().join(format!(
            "xrayc-prov-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let priv_path = dir.join("id_ed25519");
        let pub_path = dir.join("id_ed25519.pub");
        std::fs::write(&priv_path, b"PRIVATE-KEY-SENTINEL").unwrap();
        std::fs::write(&pub_path, "ssh-ed25519 EXISTINGPUB xrayc-backup\n").unwrap();

        let pubkey = ensure_backup_keypair(&dir).expect("应读已存在公钥");
        assert_eq!(
            pubkey, "ssh-ed25519 EXISTINGPUB xrayc-backup",
            "应返回去空白公钥"
        );
        assert_eq!(
            std::fs::read(&priv_path).unwrap(),
            b"PRIVATE-KEY-SENTINEL",
            "已存在时私钥不得被重生成覆盖"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn provision_rejects_non_ipv4_host() {
        let dir = std::env::temp_dir().join("xrayc-prov-nonipv4");
        let result = provision_offsite("backup.example.test", 22, "root", "pw", &dir)
            .await
            .expect("非 IPv4 应返回受控结果而非 Err");
        assert!(!result.ok, "域名 host 应被拒");
        assert!(result.fingerprint.is_none());
        assert!(result.message.contains("IPv4"), "应提示必须 IPv4");
    }
}
