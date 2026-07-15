//! 运营观测模块。
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
pub(crate) struct OperationsLedgerRankingQuery {
    pub(crate) limit: Option<i64>,
}

/// 监控中心·节点流量汇总查询参数(毫秒时间戳半开区间 [from_ms, to_ms))。
#[derive(Debug, Deserialize)]
pub(crate) struct NodeTrafficQuery {
    pub(crate) from_ms: Option<i64>,
    pub(crate) to_ms: Option<i64>,
}

/// 监控中心·单节点流量趋势查询参数(区间 + 分桶粒度 hour|day)。
#[derive(Debug, Deserialize)]
pub(crate) struct NodeTrafficTrendQuery {
    pub(crate) from_ms: Option<i64>,
    pub(crate) to_ms: Option<i64>,
    pub(crate) bucket: Option<String>,
}

pub(crate) async fn access_operations_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.access_operations_settings_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

pub(crate) async fn update_access_operations_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg.update_access_operations_settings_json(body).await {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_operations_settings.update",
                "site_settings",
                None,
                serde_json::json!({"setting_key": "access_operations"}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn trigger_access_operations_probe(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<TriggerAccessOperationsProbeRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match pg
        .record_admin_exit_endpoint_probe_request(body.exit_endpoint_id)
        .await
    {
        Ok(data) => {
            record_admin_audit(
                pg,
                &claims,
                &headers,
                "access_operations.probe",
                "exit_endpoint",
                Some(body.exit_endpoint_id),
                serde_json::json!({"manual": true}),
            )
            .await;
            Json(serde_json::json!({"success": true, "data": data})).into_response()
        }
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn operations_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if state.pg.is_some() {
        let pg = match require_admin_pg(&state, &headers).await {
            Ok(pg) => pg,
            Err(response) => return *response,
        };
        match pg.operations_summary_json().await {
            Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
            Err(err) => internal_error(err).into_response(),
        }
    } else {
        let data = state.store.read(operations_summary_json);
        Json(serde_json::json!({"success": true, "data": data})).into_response()
    }
}

pub(crate) async fn operations_ledger_ranking(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OperationsLedgerRankingQuery>,
    headers: HeaderMap,
) -> Response {
    let limit = query.limit.unwrap_or(10).clamp(1, 100);
    if state.pg.is_some() {
        let pg = match require_admin_pg(&state, &headers).await {
            Ok(pg) => pg,
            Err(response) => return *response,
        };
        match pg.operations_ledger_ranking_json(limit).await {
            Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
            Err(err) => internal_error(err).into_response(),
        }
    } else {
        let data = state
            .store
            .read(|data| operations_ledger_ranking_json(data, limit as usize));
        Json(serde_json::json!({"success": true, "data": data})).into_response()
    }
}

pub(crate) async fn access_line_sessions(
    State(state): State<Arc<AppState>>,
    Path(access_line_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.access_line_sessions_json(access_line_id).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

/// 监控中心·平台指标端点(阶段 D)。
/// 只读宿主运行态:CPU(两次 /proc/stat 短采样)、内存(/proc/meminfo)、
/// 磁盘(statvfs,host FS 根优先 /hostfs、不存在回退 /)与库存储占用,绝不改任何业务数据。
/// 仿 operations_summary:管理员鉴权失败按既有响应直接返回,无 PG 模式仍可读宿主指标 + 空库占用。
pub(crate) async fn platform_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    // PG 模式必须过管理员鉴权;无 PG 模式(纯内存)无管理库,直接读宿主指标。
    if state.pg.is_some() {
        if let Err(response) = require_admin_pg(&state, &headers).await {
            return *response;
        }
    }

    let host = collect_host_metrics().await;

    // 库存储占用:有 PG 取真实占用,无 PG 给空占用契约(total_bytes=0、空表清单)。
    let database = if let Some(pg) = &state.pg {
        match pg.database_storage_json().await {
            Ok(data) => data,
            Err(err) => return internal_error(err).into_response(),
        }
    } else {
        serde_json::json!({ "total_bytes": 0, "tables": [] })
    };

    let body = serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "cpu": { "pct_milli": host.cpu_pct_milli },
        "memory": ratio_block(host.mem_used_bytes, host.mem_total_bytes),
        "disk": disk_block(&host),
        "database": database,
    });
    Json(serde_json::json!({ "success": true, "data": body })).into_response()
}

/// 宿主运行态指标的中间结果(供 platform_metrics 组装 resp,避免 handler 里堆系统调用)。
struct HostMetrics {
    cpu_pct_milli: u32,
    mem_used_bytes: u64,
    mem_total_bytes: u64,
    disk_used_bytes: u64,
    disk_total_bytes: u64,
    disk_mount_point: String,
    disk_host_mounted: bool,
}

/// 采集一次宿主 CPU/内存/磁盘:CPU 两次 /proc/stat 间隔短采样算差;内存解析 /proc/meminfo;
/// 磁盘按 XRAYC_HOST_FS_ROOT(默认 /hostfs)statvfs,/hostfs 不存在则 host_mounted=false 回退 /。
/// 任一项读取失败按软失败处理(置 0),不让一次系统调用异常拖垮整个端点。
async fn collect_host_metrics() -> HostMetrics {
    use xrayc_core::host_metrics::{
        cpu_pct_milli, parse_proc_meminfo, parse_proc_stat, statvfs_usage,
    };

    // CPU:两次 /proc/stat 短间隔采样(150ms)作差;任一样本读失败按 0 占用兜底。
    let cpu_pct_milli = match std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|text| parse_proc_stat(&text))
    {
        Some(prev) => {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            std::fs::read_to_string("/proc/stat")
                .ok()
                .and_then(|text| parse_proc_stat(&text))
                .map(|cur| cpu_pct_milli(prev, cur))
                .unwrap_or(0)
        }
        None => 0,
    };

    // 内存:解析 /proc/meminfo;读失败按 (0,0) 兜底。
    let (mem_used_bytes, mem_total_bytes) = std::fs::read_to_string("/proc/meminfo")
        .map(|text| parse_proc_meminfo(&text))
        .unwrap_or((0, 0));

    // 磁盘:host FS 根优先 /hostfs(部署挂 / 进来),不存在则 host_mounted=false 回退 /。
    let host_fs_root =
        std::env::var("XRAYC_HOST_FS_ROOT").unwrap_or_else(|_| "/hostfs".to_string());
    let (disk_mount_point, disk_host_mounted) = if std::path::Path::new(&host_fs_root).exists() {
        (host_fs_root, true)
    } else {
        ("/".to_string(), false)
    };
    let (disk_used_bytes, disk_total_bytes) = statvfs_usage(&disk_mount_point).unwrap_or((0, 0));

    HostMetrics {
        cpu_pct_milli,
        mem_used_bytes,
        mem_total_bytes,
        disk_used_bytes,
        disk_total_bytes,
        disk_mount_point,
        disk_host_mounted,
    }
}

/// 由 used/total 字节算 used/free 千分比块。
/// 总量为 0(读 /proc 或 statvfs 软失败的兜底哨兵)时 used/free 千分比都读 0:
/// 此时占用未知,绝不报「100% free」误导面板;只有 total>0 才算真实互补千分比。
fn ratio_block(used_bytes: u64, total_bytes: u64) -> serde_json::Value {
    // checked_div 在 total=0 时返回 None,统一回落到 (0,0):占用未知不报百分比,避免除零。
    let (used_pct_milli, free_pct_milli) =
        match used_bytes.saturating_mul(100_000).checked_div(total_bytes) {
            Some(used) => {
                let used = used.min(100_000);
                (used, 100_000u64.saturating_sub(used))
            }
            None => (0, 0),
        };
    serde_json::json!({
        "used_bytes": used_bytes,
        "total_bytes": total_bytes,
        "used_pct_milli": used_pct_milli,
        "free_pct_milli": free_pct_milli,
    })
}

/// 磁盘块:在 used/free 千分比基础上补 mount_point/host_mounted 部署可观测字段。
fn disk_block(host: &HostMetrics) -> serde_json::Value {
    let mut block = ratio_block(host.disk_used_bytes, host.disk_total_bytes);
    if let Some(object) = block.as_object_mut() {
        object.insert(
            "mount_point".to_string(),
            serde_json::Value::String(host.disk_mount_point.clone()),
        );
        object.insert(
            "host_mounted".to_string(),
            serde_json::Value::Bool(host.disk_host_mounted),
        );
    }
    block
}

pub(crate) async fn access_line_metrics(
    State(state): State<Arc<AppState>>,
    Path(access_line_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.access_line_metrics_json(access_line_id).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

/// 校验并把毫秒时间戳区间解析为 UTC 时间。
/// 缺失、超出可表示范围、或 to_ms<=from_ms 一律返回清晰的 400 错误。
/// Err 用 Box<Response> 包裹,沿用本 crate(如 require_admin_pg)对大响应装箱的约定。
#[allow(clippy::type_complexity)]
fn parse_node_traffic_window(
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Result<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>), Box<Response>> {
    let from_ms =
        from_ms.ok_or_else(|| Box::new(bad_request("缺少 from_ms 参数").into_response()))?;
    let to_ms = to_ms.ok_or_else(|| Box::new(bad_request("缺少 to_ms 参数").into_response()))?;
    // to 为排他上界,必须严格大于 from,否则区间为空或非法。
    if to_ms <= from_ms {
        return Err(Box::new(
            bad_request("to_ms 必须大于 from_ms").into_response(),
        ));
    }
    let from = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(from_ms)
        .ok_or_else(|| Box::new(bad_request("from_ms 超出可表示时间范围").into_response()))?;
    let to = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(to_ms)
        .ok_or_else(|| Box::new(bad_request("to_ms 超出可表示时间范围").into_response()))?;
    Ok((from, to))
}

/// 校验分桶粒度:仅接受 hour|day,其它(含缺失)一律返回 400。
fn parse_node_traffic_bucket(bucket: Option<&str>) -> Result<&'static str, Box<Response>> {
    match bucket {
        Some("hour") => Ok("hour"),
        Some("day") => Ok("day"),
        _ => Err(Box::new(
            bad_request("bucket 仅支持 hour 或 day").into_response(),
        )),
    }
}

/// 监控中心·节点流量汇总(面板用):按中转节点聚合区间内真实上下行(delta 原始差值)。
/// 先过管理员鉴权,再校验时间区间;返回所有中转节点(零流量填 0)。
pub(crate) async fn node_traffic_summary(
    State(state): State<Arc<AppState>>,
    Query(query): Query<NodeTrafficQuery>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    let (from, to) = match parse_node_traffic_window(query.from_ms, query.to_ms) {
        Ok(window) => window,
        Err(response) => return *response,
    };
    match pg.node_traffic_summary_json(from, to).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

/// 监控中心·单节点流量趋势(详情弹窗用):按 hour|day 分桶给出每桶上下行 + 整段 summary。
/// 先过管理员鉴权,再校验时间区间与分桶粒度;节点无数据时 points 为空、summary 全 0。
pub(crate) async fn node_traffic_trend(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<Uuid>,
    Query(query): Query<NodeTrafficTrendQuery>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    let (from, to) = match parse_node_traffic_window(query.from_ms, query.to_ms) {
        Ok(window) => window,
        Err(response) => return *response,
    };
    let bucket = match parse_node_traffic_bucket(query.bucket.as_deref()) {
        Ok(bucket) => bucket,
        Err(response) => return *response,
    };
    match pg.node_traffic_trend_json(node_id, from, to, bucket).await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_node_traffic_window_rejects_missing_and_inverted_range() {
        // 缺 from_ms/to_ms 或 to<=from 都必须报错(返回 400 Response)。
        assert!(parse_node_traffic_window(None, Some(10)).is_err());
        assert!(parse_node_traffic_window(Some(10), None).is_err());
        assert!(parse_node_traffic_window(Some(10), Some(10)).is_err());
        assert!(parse_node_traffic_window(Some(20), Some(10)).is_err());
        // 合法区间应解析成功且首尾对应毫秒时间戳(不 unwrap,避免依赖 Response 的 Debug)。
        let (from, to) = match parse_node_traffic_window(Some(1_000), Some(2_000)) {
            Ok(window) => window,
            Err(_) => panic!("合法区间应解析成功"),
        };
        assert_eq!(from.timestamp_millis(), 1_000);
        assert_eq!(to.timestamp_millis(), 2_000);
    }

    #[test]
    fn test_parse_node_traffic_bucket_only_accepts_hour_or_day() {
        // 仅 hour|day 合法,其它(含缺失)一律拒绝(用 .ok() 取值,避免依赖 Response 的 Debug)。
        assert_eq!(parse_node_traffic_bucket(Some("hour")).ok(), Some("hour"));
        assert_eq!(parse_node_traffic_bucket(Some("day")).ok(), Some("day"));
        assert!(parse_node_traffic_bucket(Some("minute")).is_err());
        assert!(parse_node_traffic_bucket(Some("")).is_err());
        assert!(parse_node_traffic_bucket(None).is_err());
    }

    #[test]
    fn test_ratio_block_zero_total_is_zero_and_complementary() {
        // 总量为 0(读 /proc 或 statvfs 软失败兜底):千分比读 0,不能除零。
        let block = ratio_block(0, 0);
        assert_eq!(block["used_bytes"], 0);
        assert_eq!(block["total_bytes"], 0);
        assert_eq!(block["used_pct_milli"], 0);
        assert_eq!(block["free_pct_milli"], 0);
    }

    #[test]
    fn test_ratio_block_half_used_is_500_permille_each() {
        // used=total/2:used 千分比 50000,free 与之互补为 50000。
        let block = ratio_block(4_000, 8_000);
        assert_eq!(block["used_pct_milli"], 50_000);
        assert_eq!(block["free_pct_milli"], 50_000);
    }

    #[test]
    fn test_ratio_block_full_used_caps_at_100000() {
        // used==total:used 千分比 100000(钳上限),free 千分比 0。
        let block = ratio_block(8_000, 8_000);
        assert_eq!(block["used_pct_milli"], 100_000);
        assert_eq!(block["free_pct_milli"], 0);
    }

    #[test]
    fn test_disk_block_carries_mount_point_and_host_mounted() {
        // 磁盘块在千分比基础上补 mount_point/host_mounted 部署可观测字段。
        let host = HostMetrics {
            cpu_pct_milli: 0,
            mem_used_bytes: 0,
            mem_total_bytes: 0,
            disk_used_bytes: 2_000,
            disk_total_bytes: 8_000,
            disk_mount_point: "/hostfs".to_string(),
            disk_host_mounted: true,
        };
        let block = disk_block(&host);
        assert_eq!(block["used_pct_milli"], 25_000);
        assert_eq!(block["free_pct_milli"], 75_000);
        assert_eq!(block["mount_point"], "/hostfs");
        assert_eq!(block["host_mounted"], true);
    }
}
