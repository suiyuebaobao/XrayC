//! 一键安装远程执行器。
//! 本模块只做请求生命周期内的 SSH/SCP 调用，不持久化凭据。
//! SSH 密码通过 SSHPASS 环境变量传递，私钥写入本地临时文件后立即删除。
//! 远端 node.env 上传到临时目录，安装命令结束后删除该临时 env。
//! 任务进度仍由部署脚本回传到控制面，后台只补充排队和最终节点登记结果。
//! 返回的 stderr 会截断并脱敏，避免把服务器敏感输出写入任务表。
//! 生产路径必须使用真实 SSH；测试路径仅在 cfg(test) 下允许假输出。
//! 本模块不生成 Agent Token，鉴权码仍由服务器侧安装脚本生成。
//! 脚本来源优先使用当前工作目录 scripts，其次使用容器内 /app/scripts。
//! 文件前十行中文注释满足仓库规则。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

const DEFAULT_INSTALL_TIMEOUT_SECONDS: u64 = 1800;

pub(crate) struct RemoteAccessAgentInstallInput {
    pub(crate) ssh_host: String,
    pub(crate) ssh_port: u16,
    pub(crate) ssh_user: String,
    pub(crate) ssh_password: Option<String>,
    pub(crate) ssh_private_key: Option<String>,
    pub(crate) env_text: String,
    pub(crate) secrets_for_redaction: Vec<String>,
}

pub(crate) struct RemoteAccessAgentInstallOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stderr_summary: String,
    // 远端安装命令是否以 0 退出。非 0 不再直接判失败:只要 stdout/stderr 里解析到鉴权码,
    // 说明 agent 已起并会凭心跳收敛,调用方据此降级为成功(避免就绪等待超时/进度回报跳过误判)。
    pub(crate) succeeded: bool,
}

struct TempPath {
    path: PathBuf,
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct SshSession {
    target: String,
    port: u16,
    password: Option<String>,
    key_path: Option<PathBuf>,
    known_hosts_path: PathBuf,
}

pub(crate) async fn run_access_agent_one_click_install(
    input: RemoteAccessAgentInstallInput,
) -> Result<RemoteAccessAgentInstallOutput, String> {
    #[cfg(test)]
    if let Ok(fake_output) = std::env::var("XRAYC_ONE_CLICK_INSTALL_FAKE_OUTPUT") {
        let fake_stderr = std::env::var("XRAYC_ONE_CLICK_INSTALL_FAKE_STDERR").unwrap_or_default();
        // 测试可显式注入失败退出码,模拟"就绪等待超时但鉴权码已输出"的非 0 退出场景。
        let fake_succeeded = std::env::var("XRAYC_ONE_CLICK_INSTALL_FAKE_FAILED").is_err();
        return Ok(RemoteAccessAgentInstallOutput {
            stdout: fake_output,
            stderr_summary: fake_stderr.chars().take(1200).collect(),
            stderr: fake_stderr,
            succeeded: fake_succeeded,
        });
    }

    let script_root = deploy_script_root()?;
    let deploy_script = script_root.join("deploy-access-agent.sh");
    let deploy_lib = script_root.join("lib");
    if !deploy_script.is_file() || !deploy_lib.is_dir() {
        return Err("本地 Agent 安装脚本不存在，无法一键安装".to_string());
    }

    let install_id = Uuid::new_v4().simple().to_string();
    let remote_dir = format!("/tmp/xrayc-agent-install-{install_id}");
    let temp_dir = std::env::temp_dir().join(format!("xrayc-one-click-{install_id}"));
    fs::create_dir_all(&temp_dir).map_err(|err| format!("创建本地临时目录失败: {err}"))?;
    let _temp_dir_guard = TempPath {
        path: temp_dir.clone(),
    };

    let env_path = temp_dir.join("node.env");
    fs::write(&env_path, input.env_text.as_bytes())
        .map_err(|err| format!("写入本地临时环境文件失败: {err}"))?;
    fs::set_permissions(&env_path, fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("设置本地临时环境文件权限失败: {err}"))?;

    let key_guard = if let Some(private_key) = input.ssh_private_key.as_ref() {
        let key_path = temp_dir.join("ssh_key");
        let key_text = if private_key.ends_with('\n') {
            private_key.clone()
        } else {
            format!("{private_key}\n")
        };
        fs::write(&key_path, key_text.as_bytes())
            .map_err(|err| format!("写入本地临时 SSH 私钥失败: {err}"))?;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("设置本地临时 SSH 私钥权限失败: {err}"))?;
        Some(TempPath {
            path: key_path.clone(),
        })
    } else {
        None
    };

    let session = SshSession {
        target: format!("{}@{}", input.ssh_user, input.ssh_host),
        port: input.ssh_port,
        password: input.ssh_password.clone(),
        key_path: key_guard.as_ref().map(|guard| guard.path.clone()),
        known_hosts_path: temp_dir.join("known_hosts"),
    };
    let secrets = input.secrets_for_redaction;
    let short_timeout = Duration::from_secs(120);
    let install_timeout = Duration::from_secs(
        std::env::var("XRAYC_ONE_CLICK_INSTALL_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_INSTALL_TIMEOUT_SECONDS),
    );

    run_ssh(
        &session,
        &format!(
            "rm -rf {} && mkdir -p {}",
            shell_quote(&remote_dir),
            shell_quote(&remote_dir)
        ),
        short_timeout,
        &secrets,
    )
    .await?;
    run_scp(
        &session,
        &[deploy_script, deploy_lib],
        &scp_remote_destination(&session.target, &remote_dir),
        Duration::from_secs(300),
        &secrets,
    )
    .await?;
    run_scp(
        &session,
        &[env_path],
        &scp_remote_destination(&session.target, &format!("{remote_dir}/node.env")),
        short_timeout,
        &secrets,
    )
    .await?;

    let install_command = format!(
        "cd {} && chmod 700 ./deploy-access-agent.sh && set -a && . ./node.env && set +a && bash ./deploy-access-agent.sh; status=$?; rm -f ./node.env; exit $status",
        shell_quote(&remote_dir)
    );
    let output =
        run_ssh_capture_install(&session, &install_command, install_timeout, &secrets).await?;
    let _ = run_ssh(
        &session,
        &format!("rm -rf {}", shell_quote(&remote_dir)),
        short_timeout,
        &secrets,
    )
    .await;
    Ok(output)
}

fn deploy_script_root() -> Result<PathBuf, String> {
    if let Ok(value) = std::env::var("XRAYC_ACCESS_AGENT_DEPLOY_SCRIPT_DIR") {
        let path = PathBuf::from(value.trim());
        if path.join("deploy-access-agent.sh").is_file() {
            return Ok(path);
        }
    }
    let cwd = std::env::current_dir().map_err(|err| format!("读取当前目录失败: {err}"))?;
    for candidate in [cwd.join("scripts"), PathBuf::from("/app/scripts")] {
        if candidate.join("deploy-access-agent.sh").is_file() {
            return Ok(candidate);
        }
    }
    Err("找不到 scripts/deploy-access-agent.sh".to_string())
}

async fn run_ssh(
    session: &SshSession,
    remote_command: &str,
    timeout: Duration,
    secrets: &[String],
) -> Result<(), String> {
    run_ssh_capture(session, remote_command, timeout, secrets)
        .await
        .map(|_| ())
}

async fn run_ssh_capture(
    session: &SshSession,
    remote_command: &str,
    timeout: Duration,
    secrets: &[String],
) -> Result<RemoteAccessAgentInstallOutput, String> {
    let mut command = ssh_command(session);
    command
        .arg(&session.target)
        .arg(remote_bash_login_command(remote_command));
    let output = run_command(command, timeout).await?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(format!(
            "远程安装命令执行失败: {}",
            redact_and_truncate(&stderr, secrets, 1200)
        ));
    }
    Ok(RemoteAccessAgentInstallOutput {
        stdout: redact_text(&stdout, secrets),
        stderr: redact_text(&stderr, secrets),
        stderr_summary: redact_and_truncate(&stderr, secrets, 1200),
        succeeded: true,
    })
}

/// 专供最终安装命令的捕获:非 0 退出也返回 stdout/stderr(标 succeeded=false),
/// 不直接 Err。这样就算就绪等待超时/进度回报跳过导致脚本非 0 退出,
/// 控制面仍能从输出里解析鉴权码登记节点,避免把"已起的 agent"误判失败后强制重装。
/// 仅 SSH 进程本身无法启动/超时才返回 Err(那是真连不上,无输出可救)。
async fn run_ssh_capture_install(
    session: &SshSession,
    remote_command: &str,
    timeout: Duration,
    secrets: &[String],
) -> Result<RemoteAccessAgentInstallOutput, String> {
    let mut command = ssh_command(session);
    command
        .arg(&session.target)
        .arg(remote_bash_login_command(remote_command));
    let output = run_command(command, timeout).await?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(RemoteAccessAgentInstallOutput {
        stdout: redact_text(&stdout, secrets),
        stderr: redact_text(&stderr, secrets),
        stderr_summary: redact_and_truncate(&stderr, secrets, 16_384),
        succeeded: output.status.success(),
    })
}

async fn run_scp(
    session: &SshSession,
    sources: &[PathBuf],
    destination: &str,
    timeout: Duration,
    secrets: &[String],
) -> Result<(), String> {
    let mut command = scp_command(session);
    for source in sources {
        command.arg(source);
    }
    command.arg(destination);
    let output = run_command(command, timeout).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "上传 Agent 安装脚本失败: {}",
            redact_and_truncate(&stderr, secrets, 1200)
        ));
    }
    Ok(())
}

fn ssh_command(session: &SshSession) -> Command {
    let mut command = auth_command(session, "ssh");
    command
        .arg("-p")
        .arg(session.port.to_string())
        .args(common_ssh_options(session));
    command
}

fn scp_command(session: &SshSession) -> Command {
    let mut command = auth_command(session, "scp");
    command
        .arg("-r")
        .arg("-P")
        .arg(session.port.to_string())
        .args(common_ssh_options(session));
    command
}

fn auth_command(session: &SshSession, binary: &str) -> Command {
    let mut command = if session.password.is_some() {
        let mut command = Command::new("sshpass");
        command.arg("-e").arg(binary);
        command
    } else {
        Command::new(binary)
    };
    if let Some(password) = session.password.as_ref() {
        command.env("SSHPASS", password);
    }
    if let Some(key_path) = session.key_path.as_ref() {
        command.arg("-i").arg(key_path);
    }
    command.stdin(Stdio::null());
    command
}

fn common_ssh_options(session: &SshSession) -> Vec<String> {
    vec![
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        format!("UserKnownHostsFile={}", session.known_hosts_path.display()),
        "-o".to_string(),
        "ConnectTimeout=20".to_string(),
        "-o".to_string(),
        "ServerAliveInterval=15".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=4".to_string(),
    ]
}

/// 独立进程组只包含本次 SSH/SCP 作业，取消或超时不会遗留 sshpass 的子进程。
struct InstallProcessGroup(Option<u32>);
impl Drop for InstallProcessGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            // 该组由本次 spawn 的 process_group(0) 创建，不匹配或清理其他 SSH 会话。
            unsafe {
                libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
            }
        }
    }
}

async fn run_command(
    mut command: Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
    command
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|err| format!("启动远程安装命令失败: {err}"))?;
    let mut group = InstallProcessGroup(child.id());
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(result) => {
            group.0 = None;
            result.map_err(|err| format!("读取远程安装结果失败: {err}"))
        }
        Err(_) => Err("远程安装命令超时".to_string()),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn remote_bash_login_command(remote_command: &str) -> String {
    format!("bash -lc {}", shell_quote(remote_command))
}

fn scp_remote_destination(target: &str, remote_path: &str) -> String {
    format!("{target}:{remote_path}")
}

fn redact_and_truncate(value: &str, secrets: &[String], max_chars: usize) -> String {
    let redacted = redact_text(value, secrets);
    // 原始 stdout/stderr 仅在内存解析鉴权码；可保存的摘要必须额外遮蔽远端新生成的凭据。
    let mut parts = redacted.split("xrayc-agent-v1:");
    let mut safe = parts.next().unwrap_or_default().to_string();
    for tail in parts {
        let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        safe.push_str("[redacted-auth-code]");
        safe.push_str(&tail[end..]);
    }
    let redacted = safe;
    let chars: Vec<char> = redacted.trim().chars().collect();
    if chars.len() <= max_chars {
        return chars.into_iter().collect();
    }
    // 保留末尾真正失败原因，同时保留开头上下文；先脱敏再裁剪。
    let head = max_chars / 4;
    let tail = max_chars.saturating_sub(head + 5);
    format!(
        "{}\n[…]\n{}",
        chars[..head].iter().collect::<String>(),
        chars[chars.len() - tail..].iter().collect::<String>()
    )
}

fn redact_text(value: &str, secrets: &[String]) -> String {
    let mut redacted = value.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            redacted = redacted.replace(secret, "[redacted]");
        }
    }
    redacted
}

#[allow(dead_code)]
fn _assert_path_is_send_sync(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_timed_out_install_command_cannot_continue_in_background() {
        let marker = std::env::temp_dir().join(format!("xrayc-timeout-{}", Uuid::new_v4()));
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("sleep 1; printf unexpected > \"$1\"")
            .arg("test")
            .arg(&marker);
        let result = run_command(command, Duration::from_millis(50)).await;
        assert!(result.is_err());
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let continued = marker.exists();
        let _ = std::fs::remove_file(marker);
        assert!(!continued, "超时作业及其子进程不得继续执行");
    }

    #[test]
    fn test_saved_summary_redacts_generated_agent_auth_code() {
        let log = "节点鉴权码：xrayc-agent-v1:fixture-id:new-node-secret\nfinal-message";
        let summary = redact_and_truncate(log, &[], 16_384);
        assert!(!summary.contains("new-node-secret"));
        assert!(summary.contains("[redacted-auth-code]"));
        assert!(summary.ends_with("final-message"));
        assert!(redact_text(log, &[]).contains("new-node-secret"));
    }

    #[test]
    fn test_remote_error_keeps_final_cause_after_redaction() {
        let value = format!(
            "secret-pass{}final-download-failure secret-pass",
            "package output\n".repeat(300)
        );
        let text = redact_and_truncate(&value, &["secret-pass".to_string()], 1200);
        assert!(text.ends_with("final-download-failure [redacted]"));
        assert!(!text.contains("secret-pass"));
        assert!(text.chars().count() <= 1200);
    }

    #[test]
    fn test_remote_bash_login_command_quotes_compound_command() {
        let command =
            remote_bash_login_command("cd '/tmp/xrayc' && false; status=$?; exit $status");

        assert_eq!(
            command,
            "bash -lc 'cd '\\''/tmp/xrayc'\\'' && false; status=$?; exit $status'"
        );
    }

    #[test]
    fn test_scp_remote_destination_does_not_quote_sftp_path() {
        let destination =
            scp_remote_destination("root@example.test", "/tmp/xrayc-agent-install-abc");

        assert_eq!(
            destination,
            "root@example.test:/tmp/xrayc-agent-install-abc"
        );
    }
}
