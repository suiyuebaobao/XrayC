//! 一键安装后台 SSH 执行任务。
//! 从 deploy_install 拆出,保持单文件不超 550 行硬上限(CLAUDE.md §8)。
//! 仅承接 run_one_click_agent_install_job:SSH 上传执行脚本、解析鉴权码、登记节点、回写进度。
//! 成功判定以"鉴权码解析到"为准(agent 已起会凭心跳收敛),不据脚本退出码或进度回报跳过误判失败。
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
        .record_deployment_task_report(DeploymentTaskReportInput {
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
    let ssh_host_for_node = resolved.ssh_host.clone();
    let _ = pg
        .record_deployment_task_report(DeploymentTaskReportInput {
            task_id,
            report_token: report_token.clone(),
            status: Some("running".to_string()),
            step: Some("ssh_connect".to_string()),
            message: Some("正在自动连接服务器并上传安装脚本".to_string()),
            progress_percent: Some(15),
            result: Some(serde_json::json!({"mode": "one_click"})),
        })
        .await;

    let install_output = run_access_agent_one_click_install(RemoteAccessAgentInstallInput {
        ssh_host: resolved.ssh_host,
        ssh_port: resolved.ssh_port,
        ssh_user: resolved.ssh_user,
        ssh_password: resolved.ssh_password,
        ssh_private_key: resolved.ssh_private_key,
        env_text,
        secrets_for_redaction: secrets,
    })
    .await;

    let output = match install_output {
        Ok(output) => output,
        Err(err) => {
            // SSH 进程本身连不上/超时(无输出可救)才算硬失败。
            report_one_click_failed(&pg, task_id, &report_token, "ssh_install_failed", err).await;
            return;
        }
    };

    // 成功判定以"鉴权码解析到"为准(=agent 已起、会凭心跳回连收敛),而非脚本退出码。
    // 脚本现在在容器 running 后即先输出鉴权码,再做就绪等待;就绪等待超时/进度回报跳过只让脚本
    // 非 0 退出,但鉴权码已在输出里。只要解析到鉴权码就继续登记,把非 0 退出降级为告警,
    // 杜绝"快装好却被判 failed → 强制重装"的自伤式重装风暴。
    let extracted_auth_code =
        extract_installed_agent_auth_code_from_install_output(&output.stdout, &output.stderr);
    let Some(auth_code) = extracted_auth_code else {
        // 没解析到鉴权码才是真失败:区分"脚本非 0 退出且无鉴权码"与"退出 0 但漏打鉴权码"。
        let (step, message) = if output.succeeded {
            (
                "auth_code_missing",
                "Agent 安装完成但未输出节点鉴权码".to_string(),
            )
        } else {
            (
                "ssh_install_failed",
                format!("远程安装命令执行失败: {}", output.stderr_summary),
            )
        };
        report_one_click_failed(&pg, task_id, &report_token, step, message).await;
        return;
    };
    if !output.succeeded {
        // 解析到鉴权码但脚本非 0 退出:就绪等待超时/进度回报跳过降级为告警,继续登记节点。
        let _ = pg
            .record_deployment_task_report(DeploymentTaskReportInput {
                task_id,
                report_token: report_token.clone(),
                status: Some("running".to_string()),
                step: Some("agent_ready".to_string()),
                message: Some(
                    "就绪等待超时但已输出鉴权码;agent 将凭心跳收敛,继续登记节点".to_string(),
                ),
                progress_percent: Some(90),
                result: Some(
                    serde_json::json!({"mode": "one_click", "readiness_wait_skipped": true}),
                ),
            })
            .await;
    }
    let auth_code = match parse_access_node_auth_code(&auth_code) {
        Ok(value) => value,
        Err(err) => {
            report_one_click_failed(&pg, task_id, &report_token, "auth_code_invalid", err).await;
            return;
        }
    };
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
                .record_deployment_task_report(DeploymentTaskReportInput {
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
