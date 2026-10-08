//! 一键安装后台 SSH 执行任务。
//! 从 deploy_install 拆出,保持单文件不超 550 行硬上限(CLAUDE.md §8)。
//! 仅承接 run_one_click_agent_install_job:SSH 上传执行脚本、解析鉴权码、登记节点、回写进度。
//! 成功必须同时具备脚本成功退出、有效鉴权码及节点登记；回退或中断不得被当成成功。
//! SSH 凭据/部署 token/鉴权码只在本任务生命周期内流转,绝不落库/不回显/不入日志。
//! 失败上报统一走 report_one_click_failed,只写脱敏步骤与消息。
//! 节点登记复用 admin 节点创建/rebind 与多域名对账,不重复业务规则。
//! 不注册 HTTP 路由,只由 deploy handler 在 tokio::spawn 中调用。
//! 文件前十行中文注释满足仓库规则。
//! 请勿在源码注释或日志中写入服务器敏感信息。

use super::*;
use crate::deploy_install::*;

/// 一键安装失败上报统一收口:任务标 failed,带步骤/脱敏消息/error 结果,收敛重复失败块。
async fn report_one_click_failed(
    pg: &PgStore,
    task_id: Uuid,
    report_token: &str,
    step: &str,
    message: String,
) {
    let _ = pg
        .record_internal_deployment_task_report(DeploymentTaskReportInput {
            task_id,
            report_token: report_token.to_string(),
            status: Some("failed".to_string()),
            step: Some(step.to_string()),
            message: Some(message.clone()),
            progress_percent: Some(100),
            result: Some(serde_json::json!({"mode": "one_click", "error": message})),
        })
        .await;
}

pub(crate) async fn run_one_click_agent_install_job(
    pg: PgStore,
    task_id: Uuid,
    report_token: String,
    resolved: OneClickAgentInstallResolved,
) {
    let envs = agent_install_envs(&resolved.install, &task_id.to_string(), &report_token);
    let env_text = env_file_text(&envs);
    let mut secrets = vec![
        resolved.install.deploy_artifact_token.clone(),
        report_token.clone(),
        resolved.ssh_host.clone(),
        resolved.ssh_user.clone(),
    ];
    if let Some(value) = resolved.ssh_password.as_ref() {
        secrets.push(value.clone());
    }
    if let Some(value) = resolved.ssh_private_key.as_ref() {
        secrets.push(value.clone());
    }
    if let Some(value) = resolved.install.cf_api_token.as_ref() {
        secrets.push(value.clone());
    }
    let ssh_host_for_node = resolved.ssh_host.clone();
    let _ = pg
        .record_internal_deployment_task_report(DeploymentTaskReportInput {
            task_id,
            report_token: report_token.clone(),
            status: Some("running".to_string()),
            step: Some("ssh_connect".to_string()),
            message: Some("正在自动连接服务器并上传安装脚本".to_string()),
            progress_percent: Some(15),
            result: Some(serde_json::json!({"mode": "one_click"})),
        })
        .await;

    let install = run_access_agent_one_click_install(RemoteAccessAgentInstallInput {
        ssh_host: resolved.ssh_host,
        ssh_port: resolved.ssh_port,
        ssh_user: resolved.ssh_user,
        ssh_password: resolved.ssh_password,
        ssh_private_key: resolved.ssh_private_key,
        env_text,
        secrets_for_redaction: secrets,
    });
    tokio::pin!(install);
    let install_output = loop {
        tokio::select! {
            output = &mut install => break output,
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                match pg.touch_active_deployment_task(task_id).await {
                    Ok(false) => return,
                    Ok(true) => {},
                    Err(error) => tracing::warn!(%error, "deployment task lease refresh failed"),
                }
            }
        }
    };

    let output = match install_output {
        Ok(output) => output,
        Err(err) => {
            // SSH 进程本身连不上/超时(无输出可救)才算硬失败。
            report_one_click_failed(&pg, task_id, &report_token, "ssh_install_failed", err).await;
            return;
        }
    };

    // 脚本已把非致命就绪等待转成警告；真正的非零退出可能已触发回退，不能只凭曾打印鉴权码宣称成功。
    if !output.succeeded {
        report_one_click_failed(
            &pg,
            task_id,
            &report_token,
            "ssh_install_failed",
            format!(
                "远程安装未完成，请查看保留的服务状态与错误：{}",
                output.stderr_summary
            ),
        )
        .await;
        return;
    }
    let Some(auth_code) =
        extract_installed_agent_auth_code_from_install_output(&output.stdout, &output.stderr)
    else {
        report_one_click_failed(
            &pg,
            task_id,
            &report_token,
            "auth_code_missing",
            "安装脚本未返回有效节点鉴权码，尚未完成节点登记".to_string(),
        )
        .await;
        return;
    };
    let auth_code = match parse_access_node_auth_code(&auth_code) {
        Ok(value) => value,
        Err(err) => {
            report_one_click_failed(&pg, task_id, &report_token, "auth_code_invalid", err).await;
            return;
        }
    };
    match pg.deployment_task_allows_registration(task_id).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            report_one_click_failed(
                &pg,
                task_id,
                &report_token,
                "node_register_failed",
                error.to_string(),
            )
            .await;
            return;
        }
    }
    let acme_email = Some(resolved.acme_email).filter(|email| !email.is_empty());
    // 先算好节点登记后要对账的主域名集(安装态 rebind 路径不自行同步进 node_domains);下方 input 会 move 原值。
    let desired_domains = merge_primary_domains_into(
        &[],
        resolved.cert_domain.as_deref(),
        resolved.cf_domain.as_deref(),
        acme_email.as_deref(),
    );
    // 多模式 CF 字段:一键安装一步把节点建成 CF+直连(可带 IP 直连地址);cf_api_token 绝不进此结构。
    let input = AdminAccessNodeInput {
        name: resolved.name,
        public_host: resolved.public_host,
        public_port: resolved.public_port,
        agent_token: auth_code.agent_token,
        remark: resolved.remark,
        cert_domain: resolved.cert_domain,
        acme_email,
        cf_enabled: resolved.cf_enabled,
        cf_domain: resolved.cf_domain,
        ip_direct_address: resolved.ip_direct_address,
    };
    let created = if let Some(installed_node_id) = auth_code.installed_node_id {
        pg.rebind_admin_installed_access_node(installed_node_id, input)
            .await
    } else {
        pg.create_admin_access_node(input).await
    };
    match created {
        Ok(access_node_id) => {
            let ssh_host_update = pg
                .update_admin_access_node(
                    access_node_id,
                    AdminAccessNodeUpdate {
                        ssh_host: Some(ssh_host_for_node),
                        ..Default::default()
                    },
                )
                .await;
            if let Err(err) = ssh_host_update {
                report_one_click_failed(
                    &pg,
                    task_id,
                    &report_token,
                    "node_register_failed",
                    err.to_string(),
                )
                .await;
                return;
            }
            // 一键安装多域名对账:把主域名落进 node_domains;失败只记日志不阻断安装成功上报(节点已可用)。
            reconcile_one_click_node_domains(&pg, access_node_id, &desired_domains).await;
            let _ = pg
                .record_internal_deployment_task_report(DeploymentTaskReportInput {
                    task_id,
                    report_token: report_token.clone(),
                    status: Some("succeeded".to_string()),
                    step: Some("node_registered".to_string()),
                    message: Some("Agent 安装成功，中转节点已自动添加".to_string()),
                    progress_percent: Some(100),
                    result: Some(serde_json::json!({
                        "mode": "one_click",
                        "access_node_id": access_node_id,
                        "auth_code_received": true,
                        "stderr_summary": output.stderr_summary,
                    })),
                })
                .await;
        }
        Err(err) => {
            report_one_click_failed(
                &pg,
                task_id,
                &report_token,
                "node_register_failed",
                err.to_string(),
            )
            .await;
        }
    }
}
