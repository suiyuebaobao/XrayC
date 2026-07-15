//! Agent 安装任务的步骤进度 JSON 构造。
//! 从 deploy_install 拆出,避免该文件随多模式 CF 字段增长超过 550 行硬上限。
//! 仅返回给前端展示的步骤清单(手动安装说明 / 一键安装两套),纯数据无副作用。
//! 不读写数据库、不访问远端、不持有任何凭据。
//! 步骤文案为中文,描述安装流程各阶段状态(done/current/pending)。
//! 由 deploy_install 以 `pub(crate) use` 重导出,deploy.rs 的 glob import 无需改动。
//! 改步骤文案时与实际安装脚本阶段保持一致。
//! 一键安装比手动多「SSH 连接」「登记节点」两步(平台自动完成)。
//! 本头部满足前十行中文注释约束。

/// 手动安装说明的步骤清单(管理员在服务器上自行执行脚本)。
pub(crate) fn install_task_steps_json() -> serde_json::Value {
    serde_json::json!([
        {
            "key": "guide_generated",
            "title": "生成安装说明",
            "detail": "管理平台已生成安装环境变量和服务器执行命令。",
            "status": "done"
        },
        {
            "key": "server_started",
            "title": "服务器开始安装",
            "detail": "等待管理员在服务器上执行安装脚本。",
            "status": "current"
        },
        {
            "key": "artifacts_downloaded",
            "title": "下载部署制品",
            "detail": "服务器下载 access-agent 和 xray-core 制品。",
            "status": "pending"
        },
        {
            "key": "containers_started",
            "title": "启动运行组件",
            "detail": "服务器启动 xray 和 access-agent 容器。",
            "status": "pending"
        },
        {
            "key": "agent_ready",
            "title": "Agent 上线",
            "detail": "安装脚本输出节点鉴权码，管理员回到平台新增中转节点。",
            "status": "pending"
        }
    ])
}

/// 一键安装的步骤清单(平台自动 SSH 执行并自动登记节点)。
pub(crate) fn one_click_install_task_steps_json() -> serde_json::Value {
    serde_json::json!([
        {
            "key": "one_click_requested",
            "title": "创建安装任务",
            "detail": "管理平台已接收一键安装请求，后续由平台自动执行，SSH 凭据只在本次请求中临时使用。",
            "status": "done"
        },
        {
            "key": "ssh_connect",
            "title": "连接服务器",
            "detail": "平台自动通过 SSH 连接服务器并上传 Agent 安装脚本，无需管理员手动上传。",
            "status": "current"
        },
        {
            "key": "server_started",
            "title": "服务器开始安装",
            "detail": "平台已在服务器上自动启动安装脚本并回传进度。",
            "status": "pending"
        },
        {
            "key": "artifacts_downloaded",
            "title": "下载部署制品",
            "detail": "服务器下载 access-agent 和 xray-core 制品。",
            "status": "pending"
        },
        {
            "key": "containers_started",
            "title": "启动运行组件",
            "detail": "服务器启动 xray 和 access-agent 容器。",
            "status": "pending"
        },
        {
            "key": "agent_ready",
            "title": "Agent 上线",
            "detail": "安装脚本输出节点鉴权码。",
            "status": "pending"
        },
        {
            "key": "node_registered",
            "title": "登记中转节点",
            "detail": "管理平台解析节点鉴权码并自动添加中转节点。",
            "status": "pending"
        }
    ])
}
