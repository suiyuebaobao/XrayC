//! 代理负载解析模块。
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

/// agent 随心跳上报的本机运行态指标(与 access-agent 采集层 JSON 字段对齐)。
/// 字节列用 i64、千分比用 i32 接收,入库前由数据层钳制非法值([0,100000]/>=0)。
/// collected_at_unix 为采样 Unix 秒时间戳,数据层转 TIMESTAMPTZ 落库。
#[derive(Debug, serde::Deserialize)]
pub(crate) struct AgentNodeMetrics {
    pub(crate) cpu_pct_milli: i32,
    pub(crate) mem_used_bytes: i64,
    pub(crate) mem_total_bytes: i64,
    pub(crate) disk_used_bytes: i64,
    pub(crate) disk_total_bytes: i64,
    pub(crate) collected_at_unix: i64,
}

pub(crate) fn attach_config_refresh_fields(
    response: &mut serde_json::Value,
    refresh: serde_json::Value,
) {
    let Some(object) = response.as_object_mut() else {
        return;
    };
    for key in [
        "config_hash",
        "desired_config_version",
        "config_status",
        "config",
    ] {
        if let Some(value) = refresh.get(key) {
            object.insert(key.to_string(), value.clone());
        }
    }
}

pub(crate) fn force_config_refresh_required(response: &mut serde_json::Value, reason: &str) {
    let Some(object) = response.as_object_mut() else {
        return;
    };
    let status = object
        .entry("config_status".to_string())
        .or_insert_with(|| serde_json::json!({}));
    let Some(status_object) = status.as_object_mut() else {
        *status = serde_json::json!({
            "required": true,
            "reason": reason
        });
        return;
    };
    status_object.insert("required".to_string(), serde_json::Value::Bool(true));
    if status_object
        .get("reason")
        .and_then(|value| value.as_str())
        .is_none_or(|value| value == "up_to_date")
    {
        status_object.insert(
            "reason".to_string(),
            serde_json::Value::String(reason.to_string()),
        );
    }
    object
        .entry("config".to_string())
        .or_insert(serde_json::Value::Null);
}

pub(crate) async fn authenticated_agent_node(
    pg: &PgStore,
    headers: &HeaderMap,
    payload: &serde_json::Value,
) -> Result<Uuid, Response> {
    let reported_node_id =
        parse_required_agent_node_id_from_payload(payload).map_err(|response| *response)?;
    match require_agent_token(pg, Some(reported_node_id), headers).await {
        Ok(true) => Ok(reported_node_id),
        Ok(false) => Err(unauthorized().into_response()),
        Err(err) => Err(internal_error(err).into_response()),
    }
}

pub(crate) fn parse_required_agent_node_id(
    access_node_id: Option<Uuid>,
    node_id: Option<&str>,
) -> Result<Uuid, Box<Response>> {
    if let Some(access_node_id) = access_node_id {
        return Ok(access_node_id);
    }
    let Some(node_id) = node_id else {
        return Err(Box::new(bad_request("缺少 access_node_id").into_response()));
    };
    Uuid::parse_str(node_id)
        .map_err(|_| Box::new(bad_request("access_node_id 格式无效").into_response()))
}

pub(crate) fn parse_required_agent_node_id_from_payload(
    payload: &serde_json::Value,
) -> Result<Uuid, Box<Response>> {
    let access_node_id = payload
        .get("access_node_id")
        .or_else(|| payload.get("node_id"))
        .and_then(|value| value.as_str());
    parse_required_agent_node_id(None, access_node_id)
}

pub(crate) fn traffic_report_from_snapshot(
    snapshot: &serde_json::Value,
) -> Result<TrafficReport, Box<Response>> {
    let access_line_id = required_uuid(snapshot, "access_line_id")?;
    let xray_user_key = required_text(snapshot, "xray_user_key")?;
    let uplink_total = required_u64(snapshot, "uplink_bytes")?;
    let downlink_total = required_u64(snapshot, "downlink_bytes")?;
    let captured_at_unix = required_i64(snapshot, "captured_at_unix")?;
    let Some(collected_at) = chrono::DateTime::from_timestamp(captured_at_unix, 0) else {
        return Err(Box::new(
            bad_request("captured_at_unix 超出有效时间范围").into_response(),
        ));
    };
    Ok(TrafficReport {
        access_line_id,
        xray_user_key,
        uplink_total,
        downlink_total,
        collected_at,
    })
}

pub(crate) fn required_uuid(
    snapshot: &serde_json::Value,
    field: &str,
) -> Result<Uuid, Box<Response>> {
    let value = required_text(snapshot, field)?;
    Uuid::parse_str(&value)
        .map_err(|_| Box::new(bad_request(&format!("{field} 格式无效")).into_response()))
}

pub(crate) fn required_text(
    snapshot: &serde_json::Value,
    field: &str,
) -> Result<String, Box<Response>> {
    let Some(value) = snapshot.get(field).and_then(|value| value.as_str()) else {
        return Err(Box::new(
            bad_request(&format!("缺少 {field}")).into_response(),
        ));
    };
    let value = value.trim();
    if value.is_empty() {
        return Err(Box::new(
            bad_request(&format!("{field} 不能为空")).into_response(),
        ));
    }
    Ok(value.to_owned())
}

pub(crate) fn required_u64(
    snapshot: &serde_json::Value,
    field: &str,
) -> Result<u64, Box<Response>> {
    snapshot
        .get(field)
        .and_then(|value| value.as_u64())
        .ok_or_else(|| Box::new(bad_request(&format!("缺少 {field}")).into_response()))
}

pub(crate) fn required_i64(
    snapshot: &serde_json::Value,
    field: &str,
) -> Result<i64, Box<Response>> {
    snapshot
        .get(field)
        .and_then(|value| value.as_i64())
        .ok_or_else(|| Box::new(bad_request(&format!("缺少 {field}")).into_response()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attach_config_refresh_fields_preserves_config_fields() {
        let mut response = serde_json::json!({"success": true, "accepted": true});
        let refresh = serde_json::json!({
            "config_hash": "hash-a",
            "desired_config_version": "hash-a",
            "config_status": {"required": true},
            "config": {"node_id": "node-a"}
        });

        attach_config_refresh_fields(&mut response, refresh);

        assert_eq!(response["config_hash"], "hash-a");
        assert_eq!(response["desired_config_version"], "hash-a");
        assert_eq!(response["config"]["node_id"], "node-a");
    }
}
