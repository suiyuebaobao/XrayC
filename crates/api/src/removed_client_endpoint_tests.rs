//! 旧客户端接口下线测试。
//! 当前产品只保留传统订阅链接和普通用户订阅接口。
//! `/api/client/*` 设备、启动配置、profile 和更新检查不再注册。
//! 测试只验证路由层状态，不访问数据库或外部网络。
//! 保留这个测试是为了避免后续误把客户端设备订阅入口加回来。
//! 普通 `/sub/{token}` 订阅仍由订阅接口和端到端测试覆盖。
//! 请求体不包含真实 token、服务器地址或代理配置。
//! 新增客户端产品路线前必须重新提交方案和测试。
//! 文件前十行中文注释满足仓库规则。
//! 断言范围只限 HTTP 路由是否存在。

use super::*;
use axum::body::Body;
use axum::http::{Method, Request};
use tower::ServiceExt;

#[tokio::test]
async fn removed_official_client_routes_return_not_found() {
    let app = app(AppState::default());
    for (method, path) in [
        (Method::POST, "/api/client/devices/register"),
        (Method::GET, "/api/client/bootstrap"),
        (Method::GET, "/api/client/profile"),
        (Method::POST, "/api/client/heartbeat"),
        (Method::GET, "/api/client/update"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn removed_admin_routing_write_routes_do_not_succeed() {
    let app = app(AppState::default());
    for (method, path) in [
        (Method::POST, "/api/admin/access-lines"),
        (
            Method::PUT,
            "/api/admin/access-lines/00000000-0000-0000-0000-000000000001/exit-pool",
        ),
        (Method::POST, "/api/admin/exit-pools"),
        (
            Method::DELETE,
            "/api/admin/exit-pools/00000000-0000-0000-0000-000000000001",
        ),
        (
            Method::PUT,
            "/api/admin/exit-pools/00000000-0000-0000-0000-000000000001/members",
        ),
        (
            Method::POST,
            "/api/admin/exit-pools/00000000-0000-0000-0000-000000000001/exits",
        ),
        (
            Method::POST,
            "/api/admin/access-nodes/00000000-0000-0000-0000-000000000001/line-entries",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            matches!(
                response.status(),
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ),
            "{path} returned {}",
            response.status()
        );
    }
}
