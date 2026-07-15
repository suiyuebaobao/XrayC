//! 本模块测试配置命令失败摘要的脱敏与可诊断性（BUG-G 可观测性）。
//! 校验 `xray -test` 失败输出经摘要后：保留 Xray 错误类型行（可诊断），
//! 同时去除 token/密码/证书正文/订阅 URL 等敏感串（防泄露），且摘要非空可读。
//! 这条用例的存在动机是历史上失败原文被整段丢成"X bytes"，生产无法定位。

use super::super::apply_failure_summary::command_failure_summary;

/// 失败原文里夹带敏感串时，摘要必须保留 Xray 的错误类型行、剥掉敏感串、且非空。
#[test]
fn test_command_failure_summary_keeps_error_type_strips_secrets() {
    // 模拟 `xray -test` 失败：stderr 空、错误进 stdout（与真实 Xray 行为一致）。
    // 注：固定值全用合成占位（含 fake/test 标记，防泄露脚本可识别为非真实凭据），
    // 且 token 段足够长以触发 blob 脱敏分支；不写字面 PEM 块标记（防泄露红线另测）。
    let fake_token = "fake-token-0123456789abcdef0123456789-test";
    let fake_password = "fake-test-password-9";
    let stdout = format!(
        "Xray 26.5.9 (Xray, Penetrates Everything.)\n\
A unified platform for anti-censorship.\n\
2026/06/26 08:18:00 [Info] infra/conf/serial: Reading config\n\
2026/06/26 08:18:00 [Warning] failed to parse json config > invalid character at line 42\n\
some-leaked-token={fake_token} password: {fake_password}\n"
    );

    let summary = command_failure_summary(stdout.as_bytes(), b"");

    // 可诊断：必须保留 Xray 的错误类型行，否则等于又丢了真因。
    assert!(
        summary.contains("failed to parse json config"),
        "摘要应保留 Xray 错误类型行，实际: {summary}"
    );

    // 防泄露：敏感串（key=value/key: value 的值）必须被剥离，不能出现在摘要里。
    assert!(
        !summary.contains(fake_token),
        "摘要不应包含泄露 token，实际: {summary}"
    );
    assert!(
        !summary.contains(fake_password),
        "摘要不应包含明文密码，实际: {summary}"
    );

    // 可读：摘要非空。
    assert!(!summary.trim().is_empty(), "摘要不应为空");
}

/// stderr 非空时，同样保留错误类型行并剥敏（不再只回 "stderr redacted (N bytes)"）。
#[test]
fn test_command_failure_summary_uses_stderr_when_present() {
    let fake_token = "fake-token-0123456789abcdef-test";
    let stderr = format!(
        "reload failed: open /etc/letsencrypt/live/example.test/fullchain.pem: no such file or directory\n\
agent-token={fake_token}\n"
    );

    let summary = command_failure_summary(b"", stderr.as_bytes());

    assert!(
        summary.contains("no such file or directory"),
        "摘要应保留底层错误类型行，实际: {summary}"
    );
    assert!(
        !summary.contains(fake_token),
        "摘要不应包含泄露 token，实际: {summary}"
    );
}

/// 含 PEM 块标记的错误行整体被替换，绝不回显证书/私钥正文。
/// 标记串运行时拼接，源码里不出现字面 PEM 块（防泄露脚本红线）。
#[test]
fn test_command_failure_summary_redacts_pem_marker_line() {
    let marker = format!("-----{}-----", "BEGIN PRIVATE KEY");
    let fake_body = "fake-key-body-0123456789abcdefghij-test";
    let stdout = format!("error loading key: {marker} {fake_body}\n");

    let summary = command_failure_summary(stdout.as_bytes(), b"");

    assert!(
        !summary.contains(fake_body) && !summary.contains("BEGIN PRIVATE KEY"),
        "摘要不应回显证书/私钥正文，实际: {summary}"
    );
    assert!(!summary.trim().is_empty(), "摘要不应为空");
}

/// 两路都空时，给出可读的非空兜底（不静默）。
#[test]
fn test_command_failure_summary_empty_output_is_nonempty() {
    let summary = command_failure_summary(b"", b"");
    assert!(!summary.trim().is_empty(), "空输出也应有可读兜底文案");
}
