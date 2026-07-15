//! 本文件是 access-agent 进程入口。
//! 二进制从环境变量读取节点配置，然后启动本地 Xray 同步和流量
//! 上报运行时。

use xrayc_access_agent::config::AgentSettings;
use xrayc_access_agent::runtime::run_agent;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let settings = AgentSettings::from_env()?;
    run_agent(settings).await
}
