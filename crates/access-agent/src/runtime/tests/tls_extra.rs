//! 本模块承接从 tls.rs 拆分出来的证书签发退避（backoff）相关单元测试。
//! 仅为控制单文件行数（≤550）从 tls.rs 原样搬迁，测试逻辑、断言、名字完全不变。
//! 覆盖按需自签的失败退避窗口与成功后清退避两条纯函数行为。
//! 单元测试不执行真实 certbot，也不访问公网 ACME。
//! 退避只针对失败域名本身，别的域名不受影响。
//! 成功签发后清掉该域名失败记录，续签不被旧失败卡住。
//! 与 tls.rs 共用同一个 tls 父模块导出，helper 各自最小引入。
//! 修改本文件时继续保持测试不依赖真实服务器。
//! 真实续期由后续远端 E2E 覆盖。
//! 本头部满足前十行中文注释约束。

use super::super::tls::{should_attempt_sign, SignBackoff};

#[test]
fn test_should_attempt_sign_blocks_recent_failure_then_allows_after_backoff() {
    // 按需自签 + 失败退避:某域名签发失败后,退避窗口内不再对它重试,
    // 避免每 30s 心跳锤 Let's Encrypt 触发限频;窗口过后才放行重试。
    let mut backoff = SignBackoff::default();
    let backoff_secs: i64 = 1800; // 30 分钟退避窗口。

    // 从未失败 → 允许首次尝试。
    assert!(should_attempt_sign(
        &backoff,
        "relay.example.test",
        1_000_000,
        backoff_secs
    ));

    // 记一次失败 @1_000_000。
    backoff.record_failure("relay.example.test", 1_000_000);

    // 同一时刻/窗口内(+1000s)再尝试 → 被退避拦截。
    assert!(!should_attempt_sign(
        &backoff,
        "relay.example.test",
        1_000_000,
        backoff_secs
    ));
    assert!(!should_attempt_sign(
        &backoff,
        "relay.example.test",
        1_001_000,
        backoff_secs
    ));

    // 退避窗口过后(+1801s)→ 放行重试。
    assert!(should_attempt_sign(
        &backoff,
        "relay.example.test",
        1_001_801,
        backoff_secs
    ));

    // 退避只针对失败那个域名,别的域名不受影响。
    assert!(should_attempt_sign(
        &backoff,
        "other.example.test",
        1_000_500,
        backoff_secs
    ));
}

#[test]
fn test_record_success_clears_backoff_for_domain() {
    // 签发成功后清掉该域名的失败记录:下次到期续签不被旧失败的退避卡住。
    let mut backoff = SignBackoff::default();
    let backoff_secs: i64 = 1800;

    backoff.record_failure("relay.example.test", 1_000_000);
    assert!(!should_attempt_sign(
        &backoff,
        "relay.example.test",
        1_000_100,
        backoff_secs
    ));

    backoff.record_success("relay.example.test");
    // 清除后立即允许尝试(不必等退避窗口)。
    assert!(should_attempt_sign(
        &backoff,
        "relay.example.test",
        1_000_100,
        backoff_secs
    ));
}
