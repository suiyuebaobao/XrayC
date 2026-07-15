//! 限流模块。
//! 本文件由原 API 入口按路由域拆分而来。
//! 只移动 handler 与相关 helper，不改变路由、字段和状态码。
//! 模块保持 crate 内可见，供 lib.rs 路由装配使用。
//! 响应体、cookie、token 与审计摘要沿用原实现。
//! 数据库访问仍通过既有 PgStore 方法完成。
//! 内存模式回退逻辑保持原有分支。
//! 新增代码控制在 500 行以内便于审阅。
//! 中文注释位于文件前十行满足仓库约束。
//! 请勿在此写入部署主机、密钥或其它敏感信息。

use super::*;
pub(crate) async fn enforce_rate_limit(
    state: &AppState,
    headers: &HeaderMap,
    scope: RateLimitScope,
) -> Option<Response> {
    let identity = rate_limit_identity_hash(headers);
    if let Some(pg) = &state.pg {
        match pg
            .allow_rate_limit(
                scope.name(),
                &identity,
                scope.limit() as i32,
                scope.window().as_secs() as i64,
            )
            .await
        {
            Ok(true) => return None,
            Ok(false) => return Some(too_many_requests(scope.message()).into_response()),
            Err(error) => {
                tracing::warn!(
                    scope = scope.name(),
                    %error,
                    "PostgreSQL 共享频控失败，降级为内存频控"
                );
            }
        }
    }
    if state.rate_limiter.allow(scope, &identity) {
        None
    } else {
        Some(too_many_requests(scope.message()).into_response())
    }
}

pub(crate) fn rate_limit_identity_hash(headers: &HeaderMap) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(format!("xrayc-rate-limit:v1:{}", rate_limit_identity(headers)).as_bytes())
    )
}

pub(crate) fn rate_limit_identity(headers: &HeaderMap) -> String {
    // 只信任反代覆盖后的来源头。Caddy/反向代理必须把 X-Real-IP/X-Forwarded-For
    // 写成当前连接来源，不能把客户端自带的链路继续追加给后端。
    header_value(headers, "x-real-ip")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| forwarded_for_last_hop(headers))
        .or_else(|| header_value(headers, "cf-connecting-ip").map(str::trim))
        .filter(|value| !value.is_empty())
        .unwrap_or("direct")
        .to_string()
}

pub(crate) fn forwarded_for_last_hop(headers: &HeaderMap) -> Option<&str> {
    header_value(headers, "x-forwarded-for")?
        .rsplit(',')
        .map(str::trim)
        .find(|value| !value.is_empty())
}

pub(crate) fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}
