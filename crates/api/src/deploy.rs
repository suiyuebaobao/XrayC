//! Agent 安装入口模块。
//! 本模块同时提供手动安装说明和一键 SSH 安装两条路径。
//! 手动说明只生成模板；一键安装只在请求生命周期内使用 SSH 凭据。
//! 管理员可复制说明到服务器，也可让后台自动连接服务器、上传并执行安装脚本。
//! 部署产物下载接口继续保留，供安装脚本拉取镜像和 manifest。
//! 审计日志只记录脱敏摘要，不写入 token、服务器地址或命令明文。
//! 手动说明不得创建中转节点；一键安装成功后会用脚本鉴权码自动登记节点。
//! 鉴权码由服务器侧安装脚本生成，后台只解析并保存哈希。
//! 安装说明只负责 agent 上线，订阅入口后续通过入口管理创建。
//! 请勿在源码注释或日志中写入服务器敏感信息。

use super::*;
use crate::deploy_install::*;

const DEPLOY_ARTIFACT_TOKEN_PLACEHOLDER: &str = "请替换为制品下载鉴权码";
const CONTROL_PLANE_URL_PLACEHOLDER: &str = "https://你的控制面域名";

pub(crate) async fn agent_install_guide(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AgentInstallGuideRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let resolved = match resolve_agent_install_guide_request(pg, &headers, body).await {
        Ok(value) => value,
        Err(message) => return validation_error(&message),
    };
    let task_steps = install_task_steps_json();
    let (task, report_token) = match pg
        .create_deployment_task(CreateDeploymentTaskInput {
            kind: "agent_install".to_string(),
            target_type: "access_agent".to_string(),
            target_id: resolved.access_node_id,
            title: "Agent 安装".to_string(),
            summary: "安装说明已生成，等待服务器执行脚本并回传进度".to_string(),
            safe_metadata: serde_json::json!({
                "access_node_id": resolved.access_node_id,
                "access_node_name": resolved.access_node_name,
                "install_dir": resolved.install_dir,
                "compose_project": resolved.compose_project,
                "tls_cert_domain_count": resolved.tls_cert_domains.len(),
                "tls_cert_email_present": !resolved.acme_email.is_empty()
            }),
            steps: task_steps,
            created_by_user_id: Some(claims.user_id()),
        })
        .await
    {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };
    let task_id = task
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let envs = agent_install_envs(&resolved, &task_id, &report_token);
    let environment_text = env_file_text(&envs);
    let install_command = install_command_text(&environment_text);
    let steps = install_steps_json(&install_command);

    record_admin_audit_with_result(
        pg,
        &claims,
        &headers,
        AdminAuditRecord {
            action: "access_node.agent_install_guide",
            resource_type: "access_node",
            resource_id: resolved.access_node_id,
            request_summary: serde_json::json!({
                "guide_id": resolved.guide_id,
                "deployment_task_id": task.get("id").and_then(serde_json::Value::as_str),
                "access_node_id": resolved.access_node_id,
                "access_node_name": resolved.access_node_name,
                "agent_credentials_generated_by_script": true,
                "deploy_artifact_token_placeholder": true,
                "expected_listen_port_count": resolved.expected_listen_ports.len(),
                "tls_cert_domain_count": resolved.tls_cert_domains.len(),
                "tls_cert_email_present": !resolved.acme_email.is_empty(),
                "disable_legacy_systemd_units": resolved.disable_legacy_systemd_units,
                "force_reinstall": resolved.force_reinstall,
            }),
            result: "guide_generated",
        },
    )
    .await;

    Json(serde_json::json!({
        "success": true,
        "data": {
            "guide_id": resolved.guide_id,
            "status": "ready",
            "summary": "安装说明已生成；服务器安装成功后会输出节点鉴权码，再回到平台新增中转节点",
            "access_node_id": resolved.access_node_id,
            "access_node_name": resolved.access_node_name,
            "install_dir": resolved.install_dir,
            "compose_project": resolved.compose_project,
            "script_source_hint": "将项目中的 scripts/deploy-access-agent.sh 和 scripts/lib/deploy-access-agent/ 上传到服务器同一目录后执行。",
            "environment": envs.iter().map(|(name, value)| {
                serde_json::json!({
                    "name": name,
                    "value": value,
                    "secret": matches!(name.as_str(), "XRAYC_DEPLOY_ARTIFACT_TOKEN" | "XRAYC_DEPLOY_TASK_REPORT_TOKEN"),
                })
            }).collect::<Vec<_>>(),
            "environment_text": environment_text,
            "install_command": install_command,
            "steps": steps,
            "task": task,
            "report_token": report_token,
        }
    }))
    .into_response()
}

pub(crate) async fn one_click_agent_install(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<OneClickAgentInstallRequest>,
) -> Response {
    let (pg, claims) = match require_admin_pg_with_claims(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let resolved = match resolve_one_click_agent_install_request(&headers, body).await {
        Ok(value) => value,
        Err(message) => return validation_error(&message),
    };
    // 重装熔断(CLAUDE.md §13 防自伤式重装风暴):一键安装后端一律 force_reinstall=true,会清掉
    // 已有 agent 重来。若同一 SSH 目标在熔断窗口内强制重装次数已达上限,拒绝本次,避免"见 failed
    // 就无条件重装 → 把快装好的 agent 清掉重来 → 节点永不收敛"。窗口/上限可经 env 调,默认 10 分钟 3 次。
    let reinstall_window = env_nonempty("XRAYC_ONE_CLICK_REINSTALL_WINDOW_SECONDS")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(600);
    let reinstall_limit = env_nonempty("XRAYC_ONE_CLICK_REINSTALL_LIMIT")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(3);
    match pg
        .count_recent_forced_reinstalls_for_ssh_host(&resolved.ssh_host, reinstall_window)
        .await
    {
        Ok(recent) if recent >= reinstall_limit => {
            return validation_error(&format!(
                "该服务器近 {} 分钟内强制重装已达 {} 次上限,已熔断;请确认 agent 是否已凭心跳上线,或稍后再试,避免反复重装导致节点永不收敛",
                (reinstall_window + 59) / 60,
                reinstall_limit
            ));
        }
        Ok(_) => {}
        // 熔断统计本身查询失败不阻断安装(放行),只避免误杀正常安装路径。
        Err(err) => tracing::warn!("重装熔断统计查询失败,放行本次一键安装: {err}"),
    }
    let task_steps = one_click_install_task_steps_json();
    let (task, report_token) = match pg
        .create_deployment_task(CreateDeploymentTaskInput {
            kind: "agent_install".to_string(),
            target_type: "access_agent".to_string(),
            target_id: None,
            title: "Agent 一键安装".to_string(),
            summary: "平台正在自动连接服务器、上传并执行 Agent 安装脚本".to_string(),
            safe_metadata: serde_json::json!({
                "mode": "one_click",
                "access_node_name": &resolved.name,
                "public_host": &resolved.public_host,
                "public_port": resolved.public_port,
                "ssh_host": &resolved.ssh_host,
                "install_dir": &resolved.install.install_dir,
                "compose_project": &resolved.install.compose_project,
                "ssh_port": resolved.ssh_port,
                "ssh_auth_method": if resolved.ssh_private_key.is_some() { "private_key" } else { "password" },
                "tls_cert_domain_count": resolved.install.tls_cert_domains.len(),
                "tls_cert_email_present": !resolved.install.acme_email.is_empty(),
                "force_reinstall": resolved.install.force_reinstall,
            }),
            steps: task_steps,
            created_by_user_id: Some(claims.user_id()),
        })
        .await
    {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };
    let task_id = task
        .get("id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok());
    let Some(task_id) = task_id else {
        return internal_error("部署任务 ID 无效").into_response();
    };
    let task = match pg
        .record_deployment_task_report(DeploymentTaskReportInput {
            task_id,
            report_token: report_token.clone(),
            status: Some("running".to_string()),
            step: Some("one_click_requested".to_string()),
            message: Some("一键安装任务已创建，平台正在准备自动连接服务器".to_string()),
            progress_percent: Some(5),
            result: Some(serde_json::json!({"mode": "one_click"})),
        })
        .await
    {
        Ok(value) => value,
        Err(err) => return internal_error(err).into_response(),
    };

    record_admin_audit_with_result(
        pg,
        &claims,
        &headers,
        AdminAuditRecord {
            action: "access_node.agent_one_click_install",
            resource_type: "access_node",
            resource_id: None,
            request_summary: serde_json::json!({
                "deployment_task_id": task_id,
                "access_node_name": &resolved.name,
                "public_host": &resolved.public_host,
                "public_port": resolved.public_port,
                "ssh_port": resolved.ssh_port,
                "ssh_credentials_redacted": true,
                "tls_cert_domain_count": resolved.install.tls_cert_domains.len(),
                "tls_cert_email_present": !resolved.install.acme_email.is_empty(),
                "force_reinstall": resolved.install.force_reinstall,
            }),
            result: "queued",
        },
    )
    .await;

    let pg_for_job = pg.clone();
    tokio::spawn(async move {
        run_one_click_agent_install_job(pg_for_job, task_id, report_token, resolved).await;
    });

    Json(serde_json::json!({
        "success": true,
        "data": {
            "status": "queued",
            "summary": "Agent 一键安装任务已创建；平台会自动连接服务器、上传并执行脚本，无需手动上传，成功后自动添加中转节点。",
            "task": task,
        }
    }))
    .into_response()
}

pub(crate) async fn deployment_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.deployment_tasks_json().await {
        Ok(data) => Json(serde_json::json!({"success": true, "data": data})).into_response(),
        Err(err) => internal_error(err).into_response(),
    }
}

/// 管理员删除任意部署任务记录,用于清理失败/卡死残留。
pub(crate) async fn delete_deployment_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<Uuid>,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.delete_deployment_task(task_id).await {
        Ok(()) => Json(serde_json::json!({"success": true})).into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

/// 管理员取消未完成部署任务:非终态标记 failed,终态原样返回不回退。
pub(crate) async fn cancel_deployment_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<Uuid>,
) -> Response {
    let pg = match require_admin_pg(&state, &headers).await {
        Ok(pg) => pg,
        Err(response) => return *response,
    };
    match pg.cancel_deployment_task(task_id).await {
        Ok(task) => Json(serde_json::json!({"success": true, "data": task})).into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn report_deployment_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<DeploymentTaskReportRequest>,
) -> Response {
    let Some(pg) = state.pg.as_ref() else {
        return internal_error("deployment task report requires PostgreSQL").into_response();
    };
    match pg
        .record_deployment_task_report(DeploymentTaskReportInput {
            task_id,
            report_token: body.report_token,
            status: body.status,
            step: body.step,
            message: body.message,
            progress_percent: body.progress_percent,
            result: body.result,
        })
        .await
    {
        Ok(task) => Json(serde_json::json!({"success": true, "data": task})).into_response(),
        Err(err) => unprocessable(err).into_response(),
    }
}

pub(crate) async fn resolve_agent_install_guide_request(
    pg: &PgStore,
    headers: &HeaderMap,
    body: AgentInstallGuideRequest,
) -> Result<AgentInstallGuideResolved, String> {
    let guide_id = Uuid::new_v4();
    let (
        access_node_id,
        access_node_name,
        inferred_tls_cert_domains,
        node_cf_domain,
        node_cf_cert_mode,
    ) = if let Some(access_node_id) = body.access_node_id {
        let summary = pg
            .admin_access_node_summary_json(access_node_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "中转节点不存在".to_string())?;
        let access_node_name = summary
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("access-node")
            .to_string();
        let mut inferred_tls_cert_domains = pg
            .access_node_tls_cert_domains(access_node_id)
            .await
            .map_err(|error| error.to_string())?;
        // 启用 CF 且配置了直连证书域名时,把 cert_domain 并入 certbot 申请清单;
        // 同时捕获 cf_domain/cf_cert_mode 注入 agent env,使已有节点的安装也能走 DNS-01。
        let mut node_cf_domain: Option<String> = None;
        let mut node_cf_cert_mode = "reuse_direct".to_string();
        if let Some(fields) = pg
            .access_node_cf_fields(access_node_id)
            .await
            .map_err(|error| error.to_string())?
        {
            if fields.cf_enabled {
                node_cf_domain = fields.cf_domain.clone();
                node_cf_cert_mode = fields.cf_cert_mode.clone();
                if let Some(cert_domain) = fields.cert_domain {
                    let cert_domain = cert_domain.trim().to_string();
                    if !cert_domain.is_empty() && !inferred_tls_cert_domains.contains(&cert_domain)
                    {
                        inferred_tls_cert_domains.push(cert_domain);
                    }
                }
            }
        }
        (
            Some(access_node_id),
            access_node_name,
            inferred_tls_cert_domains,
            node_cf_domain,
            node_cf_cert_mode,
        )
    } else {
        (
            None,
            "agent-first-install".to_string(),
            Vec::new(),
            None,
            "reuse_direct".to_string(),
        )
    };

    // 控制面地址优先级(CLAUDE.md §13):后端注入的稳定公网中心地址 XRAYC_PUBLIC_BASE_URL 最高,
    // 其次请求体传入值(前端浏览器 origin 在本机/IP/备用域名下远端 agent 不可达),再回退请求头推断,
    // 都为空才用占位提示。保证安装制品下载/回连指向稳定可达的中心地址。
    let control_plane_url = env_nonempty("XRAYC_PUBLIC_BASE_URL")
        .or(optional_request_text(
            body.control_plane_url,
            "control_plane_url",
            2048,
        )?)
        .or_else(|| infer_control_plane_url(headers))
        .unwrap_or_else(|| CONTROL_PLANE_URL_PLACEHOLDER.to_string());
    let deploy_artifact_token = DEPLOY_ARTIFACT_TOKEN_PLACEHOLDER.to_string();
    let expected_listen_ports = validate_expected_listen_ports(body.expected_listen_ports)?;
    let acme_email = validate_acme_email(body.acme_email)?;

    Ok(AgentInstallGuideResolved {
        guide_id,
        access_node_id,
        access_node_name,
        control_plane_url,
        deploy_artifact_token,
        acme_email,
        install_dir: optional_request_text(body.install_dir, "install_dir", 512)?
            .unwrap_or_else(|| "/opt/xrayc/access-agent".to_string()),
        compose_project: optional_request_text(body.compose_project, "compose_project", 128)?
            .unwrap_or_else(|| "xrayc-access".to_string()),
        xray_api_server: optional_request_text(body.xray_api_server, "xray_api_server", 128)?
            .unwrap_or_else(|| "127.0.0.1:10085".to_string()),
        xray_api_listen_host: optional_request_text(
            body.xray_api_listen_host,
            "xray_api_listen_host",
            128,
        )?
        .unwrap_or_else(|| "127.0.0.1".to_string()),
        xray_api_listen_port: require_nonzero_port(
            body.xray_api_listen_port.unwrap_or(10085),
            "xray_api_listen_port",
        )?,
        tls_cert_domains: normalize_tls_cert_domains(
            body.tls_cert_domains,
            inferred_tls_cert_domains,
        )?,
        heartbeat_interval_seconds: body.heartbeat_interval_seconds.unwrap_or(30),
        traffic_interval_seconds: body.traffic_interval_seconds.unwrap_or(60),
        session_idle_seconds: body.session_idle_seconds.unwrap_or(180),
        expected_listen_ports,
        disable_legacy_systemd_units: body.disable_legacy_systemd_units,
        force_reinstall: body.force_reinstall,
        // CF API Token 仅注入 agent env 给 certbot DNS-01,此处只裁剪空白,不落库/不回显。
        cf_api_token: optional_request_text(body.cf_api_token, "cf_api_token", 4096)?,
        // CF 域名与证书模式从节点 cf_fields 捕获,注入 agent env 触发 DNS-01。
        cf_domain: node_cf_domain,
        cf_cert_mode: node_cf_cert_mode,
    })
}
