//! 接入代理接口模块。
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
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct HeartbeatRequest {
    pub(crate) access_node_id: Option<Uuid>,
    pub(crate) node_id: Option<String>,
    pub(crate) agent_version: Option<String>,
    pub(crate) xray_version: Option<String>,
    pub(crate) applied_config_hash: Option<String>,
    pub(crate) applied_config_version: Option<String>,
    #[serde(default)]
    pub(crate) tls_certificates: Vec<AgentTlsCertificateReport>,
    pub(crate) tls_renew_result: Option<AgentTlsRenewResult>,
    // 内核能力软信号(§7.7.1):缺省即旧 agent 不上报,按可用/无待重启兜底落库。
    pub(crate) kernel_connmark_available: Option<bool>,
    pub(crate) kernel_upgrade_pending: Option<bool>,
    // 整机重启执行结果:agent 收到 reboot 命令、做完安全自检后回报,据 request_id 清理请求。
    pub(crate) reboot_result: Option<AgentRebootResult>,
    // 监控中心·节点运行态指标(阶段 C):agent 随心跳上报本机 CPU/内存/磁盘占用,
    // 缺省即旧 agent 不上报,按 None 处理(向后兼容,不写时序表)。DTO 见 agent_payload。
    pub(crate) node_metrics: Option<AgentNodeMetrics>,
}

/// agent 回报的整机重启执行结果(与 client DTO 对齐;status∈{success,running,failed})。
#[derive(Debug, Deserialize)]
pub(crate) struct AgentRebootResult {
    pub(crate) request_id: Uuid,
    pub(crate) status: String,
    pub(crate) message: String,
}

pub(crate) async fn agent_heartbeat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<HeartbeatRequest>,
) -> Response {
    let applied = body
        .applied_config_hash
        .as_ref()
        .or(body.applied_config_version.as_ref())
        .map(String::as_str);

    if let Some(pg) = &state.pg {
        let reported_node_id =
            match parse_required_agent_node_id(body.access_node_id, body.node_id.as_deref()) {
                Ok(node_id) => node_id,
                Err(response) => return *response,
            };
        match require_agent_token(pg, Some(reported_node_id), &headers).await {
            Ok(true) => {}
            Ok(false) => return unauthorized().into_response(),
            Err(err) => return internal_error(err).into_response(),
        }
        if let Err(err) = pg
            .record_agent_versions(
                reported_node_id,
                body.agent_version.as_deref(),
                body.xray_version.as_deref(),
            )
            .await
        {
            return unprocessable(err).into_response();
        }
        if let Err(err) = pg
            .record_agent_tls_status(
                reported_node_id,
                body.tls_certificates,
                body.tls_renew_result,
            )
            .await
        {
            return unprocessable(err).into_response();
        }
        // 落库内核能力软状态 + 整机重启结果(§7.7.1):缺省字段按可用/无待重启兜底,
        // 重启结果据 request_id 幂等清理待执行请求。失败按可处理实体错误返回。
        if let Err(err) = pg
            .record_agent_kernel_and_reboot_status(
                reported_node_id,
                body.kernel_connmark_available,
                body.kernel_upgrade_pending,
                body.reboot_result
                    .map(|result| (result.request_id, result.status, result.message)),
            )
            .await
        {
            return unprocessable(err).into_response();
        }
        // 监控中心·节点运行态指标入库(阶段 C):仅当 agent 上报 node_metrics 才写时序表,
        // 缺省(旧 agent)不写,保持心跳向后兼容;写入侧由数据层钳制非法值,失败按可处理实体返回。
        if let Some(metrics) = body.node_metrics {
            if let Err(err) = pg
                .record_node_runtime_metrics(
                    reported_node_id,
                    NodeRuntimeMetricsReport {
                        cpu_pct_milli: metrics.cpu_pct_milli,
                        mem_used_bytes: metrics.mem_used_bytes,
                        mem_total_bytes: metrics.mem_total_bytes,
                        disk_used_bytes: metrics.disk_used_bytes,
                        disk_total_bytes: metrics.disk_total_bytes,
                        collected_at_unix: metrics.collected_at_unix,
                    },
                )
                .await
            {
                return unprocessable(err).into_response();
            }
        }
        match pg.heartbeat_json(Some(reported_node_id), applied).await {
            Ok(data) => Json(data).into_response(),
            Err(err) => internal_error(err).into_response(),
        }
    } else {
        let reported_node_id =
            match parse_required_agent_node_id(body.access_node_id, body.node_id.as_deref()) {
                Ok(node_id) => node_id,
                Err(response) => return *response,
            };
        let data = state
            .store
            .read(|data| heartbeat_json(data, Some(reported_node_id), applied));
        Json(data).into_response()
    }
}

pub(crate) async fn agent_traffic(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let applied_config_version = payload
        .get("applied_config_hash")
        .or_else(|| payload.get("applied_config_version"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let reported_node_id = match parse_required_agent_node_id_from_payload(&payload) {
        Ok(node_id) => node_id,
        Err(response) => return *response,
    };
    if let Some(pg) = &state.pg {
        match require_agent_token(pg, Some(reported_node_id), &headers).await {
            Ok(true) => {}
            Ok(false) => return unauthorized().into_response(),
            Err(err) => return internal_error(err).into_response(),
        }
    }
    let reported_node_id = Some(reported_node_id);

    let report = if payload.get("snapshots").is_some() {
        let Some(snapshots) = payload
            .get("snapshots")
            .and_then(|snapshots| snapshots.as_array())
        else {
            return bad_request("snapshots 必须是数组").into_response();
        };

        let mut results = Vec::with_capacity(snapshots.len());
        for snapshot in snapshots {
            let report = match traffic_report_from_snapshot(snapshot) {
                Ok(report) => report,
                Err(response) => return *response,
            };
            let result = if let (Some(pg), Some(reported_node_id)) = (&state.pg, reported_node_id) {
                match pg
                    .apply_report_for_node(Some(reported_node_id), report)
                    .await
                {
                    Ok(result) => result,
                    Err(err) if should_discard_stale_traffic_snapshot(&err) => {
                        discarded_traffic_report_result()
                    }
                    Err(err) => return unprocessable(err).into_response(),
                }
            } else {
                match state.traffic.apply_report(report) {
                    Ok(result) => result,
                    Err(err) => return unprocessable(err).into_response(),
                }
            };
            results.push(result);
        }
        let refresh_required = results.iter().any(|result| result.config_refresh_required);
        let mut response = serde_json::json!({
            "success": true,
            "accepted": true,
            "data": {
                "accepted": true,
                "results": results
            }
        });
        if refresh_required {
            match agent_config_refresh_payload(
                &state,
                reported_node_id,
                applied_config_version.as_deref(),
            )
            .await
            {
                Ok(refresh) => {
                    let should_apply = refresh_payload_requires_apply(&refresh);
                    attach_config_refresh_fields(&mut response, refresh);
                    if should_apply {
                        force_config_refresh_required(&mut response, "traffic_refresh_required");
                    }
                }
                Err(response) => return response,
            }
        }
        return Json(response).into_response();
    } else {
        match serde_json::from_value::<TrafficReport>(payload) {
            Ok(report) => report,
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"success": false, "message": error.to_string()})),
                )
                    .into_response();
            }
        }
    };

    let result = if let (Some(pg), Some(reported_node_id)) = (&state.pg, reported_node_id) {
        pg.apply_report_for_node(Some(reported_node_id), report)
            .await
            .map_err(|err| err.to_string())
    } else {
        state
            .traffic
            .apply_report(report)
            .map_err(|err| err.to_string())
    };

    match result {
        Ok(result) => {
            let refresh_required = result.config_refresh_required;
            let mut response = serde_json::json!({
                "success": true,
                "accepted": true,
                "data": result
            });
            if refresh_required {
                match agent_config_refresh_payload(
                    &state,
                    reported_node_id,
                    applied_config_version.as_deref(),
                )
                .await
                {
                    Ok(refresh) => {
                        let should_apply = refresh_payload_requires_apply(&refresh);
                        attach_config_refresh_fields(&mut response, refresh);
                        if should_apply {
                            force_config_refresh_required(
                                &mut response,
                                "traffic_refresh_required",
                            );
                        }
                    }
                    Err(response) => return response,
                }
            }
            Json(response).into_response()
        }
        Err(err) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"success": false, "message": err})),
        )
            .into_response(),
    }
}

fn refresh_payload_requires_apply(refresh: &serde_json::Value) -> bool {
    refresh
        .get("config_status")
        .and_then(|status| status.get("required"))
        .and_then(|required| required.as_bool())
        .unwrap_or(false)
        || refresh
            .get("config")
            .is_some_and(|config| !config.is_null())
}

fn should_discard_stale_traffic_snapshot(err: &DbError) -> bool {
    matches!(
        err,
        DbError::AccessLineNotFound | DbError::AccessLineNodeMismatch | DbError::UserNotFound
    )
}

fn discarded_traffic_report_result() -> TrafficReportResult {
    // 旧积压可能引用已删除线路或旧用户；丢弃单条，避免刷新 Xray 导致 stats 反复归零。
    TrafficReportResult {
        accepted: false,
        baseline_only: false,
        delta_uplink: 0,
        delta_downlink: 0,
        billed_bytes: 0,
        config_refresh_required: false,
    }
}

pub(crate) async fn agent_config_refresh_payload(
    state: &Arc<AppState>,
    reported_node_id: Option<Uuid>,
    applied_config_version: Option<&str>,
) -> Result<serde_json::Value, Response> {
    if let Some(pg) = &state.pg {
        let Some(reported_node_id) = reported_node_id else {
            return Ok(serde_json::json!({
                "config_status": {"required": false, "reason": "missing_node_id"},
                "config": null
            }));
        };
        pg.heartbeat_json(Some(reported_node_id), applied_config_version)
            .await
            .map_err(|err| internal_error(err).into_response())
    } else {
        Ok(state
            .store
            .read(|data| heartbeat_json(data, reported_node_id, applied_config_version)))
    }
}

pub(crate) async fn agent_config_result(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    if let Some(pg) = &state.pg {
        let reported_node_id = match parse_required_agent_node_id_from_payload(&payload) {
            Ok(node_id) => node_id,
            Err(response) => return *response,
        };
        match require_agent_token(pg, Some(reported_node_id), &headers).await {
            Ok(true) => {}
            Ok(false) => return unauthorized().into_response(),
            Err(err) => return internal_error(err).into_response(),
        }
        let success = payload
            .get("success")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let config_version = payload
            .get("config_version")
            .or_else(|| payload.get("config_hash"))
            .or_else(|| payload.get("desired_config_version"))
            .and_then(|value| value.as_str());
        let message = payload.get("message").and_then(|value| value.as_str());
        if let Err(err) = pg
            .record_config_result(Some(reported_node_id), config_version, success, message)
            .await
        {
            return internal_error(err).into_response();
        }
    }

    Json(serde_json::json!({
        "success": true,
        "accepted": true
    }))
    .into_response()
}

pub(crate) async fn agent_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    if let Some(pg) = &state.pg {
        let reported_node_id = match authenticated_agent_node(pg, &headers, &payload).await {
            Ok(node_id) => node_id,
            Err(response) => return response,
        };
        match pg.record_agent_metrics(reported_node_id, &payload).await {
            Ok(written) => {
                return Json(serde_json::json!({
                    "success": true,
                    "accepted": true,
                    "data": {
                        "accepted": true,
                        "written": written
                    }
                }))
                .into_response();
            }
            Err(err) => return unprocessable(err).into_response(),
        }
    }

    Json(serde_json::json!({
        "success": true,
        "accepted": true,
        "data": {
            "accepted": true,
            "written": 0
        }
    }))
    .into_response()
}

pub(crate) async fn agent_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    if let Some(pg) = &state.pg {
        let reported_node_id = match authenticated_agent_node(pg, &headers, &payload).await {
            Ok(node_id) => node_id,
            Err(response) => return response,
        };
        match pg.record_agent_sessions(reported_node_id, &payload).await {
            Ok(written) => {
                return Json(serde_json::json!({
                    "success": true,
                    "accepted": true,
                    "data": {
                        "accepted": true,
                        "written": written
                    }
                }))
                .into_response();
            }
            Err(err) => return unprocessable(err).into_response(),
        }
    }

    Json(serde_json::json!({
        "success": true,
        "accepted": true,
        "data": {
            "accepted": true,
            "written": 0
        }
    }))
    .into_response()
}

pub(crate) async fn agent_probes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    if let Some(pg) = &state.pg {
        let reported_node_id = match authenticated_agent_node(pg, &headers, &payload).await {
            Ok(node_id) => node_id,
            Err(response) => return response,
        };
        match pg.record_agent_probes(reported_node_id, &payload).await {
            Ok(data) => {
                return Json(serde_json::json!({
                    "success": true,
                    "accepted": true,
                    "data": data
                }))
                .into_response();
            }
            Err(err) => return unprocessable(err).into_response(),
        }
    }

    Json(serde_json::json!({
        "success": true,
        "accepted": true,
        "data": {
            "accepted": true,
            "line_probes": 0,
            "exit_probes": 0
        }
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stale_traffic_snapshot_errors_are_discardable() {
        assert!(should_discard_stale_traffic_snapshot(
            &DbError::AccessLineNotFound
        ));
        assert!(should_discard_stale_traffic_snapshot(
            &DbError::AccessLineNodeMismatch
        ));
        assert!(should_discard_stale_traffic_snapshot(
            &DbError::UserNotFound
        ));
    }

    #[test]
    fn test_discarded_traffic_report_result_does_not_restart_xray() {
        let result = discarded_traffic_report_result();

        assert!(!result.accepted);
        assert!(!result.baseline_only);
        assert_eq!(result.delta_uplink, 0);
        assert_eq!(result.delta_downlink, 0);
        assert_eq!(result.billed_bytes, 0);
        assert!(!result.config_refresh_required);
    }

    #[test]
    fn test_refresh_payload_does_not_require_apply_when_already_up_to_date() {
        let refresh = serde_json::json!({
            "config_status": {
                "required": false,
                "reason": "up_to_date"
            },
            "config": null
        });

        assert!(!refresh_payload_requires_apply(&refresh));
    }

    #[test]
    fn test_refresh_payload_requires_apply_when_config_is_present() {
        let refresh = serde_json::json!({
            "config_status": {
                "required": false,
                "reason": "traffic_refresh_required"
            },
            "config": {
                "version": "v1",
                "inbounds": []
            }
        });

        assert!(refresh_payload_requires_apply(&refresh));
    }

    #[test]
    fn test_refresh_payload_requires_apply_when_status_requires_it() {
        let refresh = serde_json::json!({
            "config_status": {
                "required": true,
                "reason": "unknown_applied_version"
            },
            "config": null
        });

        assert!(refresh_payload_requires_apply(&refresh));
    }
}
