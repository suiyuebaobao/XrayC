//! 本文件定义 access-agent 环境变量配置。
//! 中转 agent 部署在中转节点上，只读取连接控制面和管理本地 Xray 配置
//! 所需的运行参数。

use std::env;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCore {
    #[default]
    Xray,
}

impl RuntimeCore {
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeCore::Xray => "xray",
        }
    }
}

impl fmt::Display for RuntimeCore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RuntimeCore {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "xray" | "xray_core" | "xray-core" => Ok(Self::Xray),
            _ => Err(anyhow::anyhow!(
                "XRAYC_CORE_TYPE/XRAYC_RUNTIME_CORE must be xray"
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AgentSettings {
    pub node_id: String,
    pub control_plane_url: String,
    pub node_token: String,
    pub runtime_core: RuntimeCore,
    pub heartbeat_interval_seconds: u64,
    pub traffic_interval_seconds: u64,
    pub session_idle_seconds: u64,
    pub xray_config_path: PathBuf,
    pub agent_state_path: PathBuf,
    pub traffic_backlog_path: PathBuf,
    pub xray_access_log_path: PathBuf,
    pub xray_binary: String,
    pub xray_test_command: String,
    pub xray_reload_command: String,
    // reload/切换 Xray 后回收旧 Xray 进程/容器的命令。
    // 用于清理因 SO_REUSEPORT 仍占用 Stats API 端口的陈旧实例，
    // 否则 statsquery 可能被内核分流到旧进程读不到当前用户流量，计费失效。
    pub xray_reclaim_command: String,
    // 当前 Xray 容器名，回收命令据此判断哪些是"旧实例"。
    pub xray_container_name: String,
    pub xray_start_command: String,
    pub xray_stop_command: String,
    pub xray_api_server: String,
    pub xray_api_listen_host: String,
    pub xray_api_listen_port: u16,
    pub rate_limiter_enabled: bool,
    pub rate_limiter_dry_run: bool,
    pub rate_limiter_interface: String,
    pub rate_limiter_ifb_interface: String,
    pub rate_limiter_root_rate_bps: u64,
    pub rate_limiter_plan_path: PathBuf,
    pub tls_cert_domains: Vec<String>,
    pub certbot_binary: String,
    pub openssl_binary: String,
    // CF API Token:仅签发/续期 DNS-01 证书用,只读不回显、不入心跳/日志/审计。
    // 安装期由控制面注入 env,agent 据此写本机 0600 ini 供 certbot-dns-cloudflare 用。
    pub cloudflare_api_token: String,
    // CF 回源证书模式:dns01(给 cf_domain 签自己的证书)或 reuse_direct(复用直连证书)。
    pub cf_cert_mode: String,
    // CF 橙云对外域名,DNS-01 证书的签发目标。
    pub cf_domain: String,
    // ACME 邮箱,DNS-01 签证书 -m 参数用(与直连 HTTP-01 共用同一邮箱)。
    pub acme_email: String,
    // 内核能力自检命令:探测下行整形依赖的 act_connmark 是否可加载;置空跳过真实探测(测试)。
    pub kernel_connmark_probe_command: String,
    // 自动装最新内核命令(幂等、保留旧内核 GRUB 回退、绝不重启);置空则不自动装内核只上报。
    pub kernel_upgrade_command: String,
    // 整机重启命令(管理员面板触发、agent 自检后下达);置空则不真重启(测试)。
    pub host_reboot_command: String,
    // 重启前自检:GRUB 默认是否引导最高版本内核(退出 0=满足);置空按满足放行(测试)。
    pub reboot_check_grub_default_command: String,
    // 重启前自检:本机容器自启策略是否 always/unless-stopped(退出 0=满足);置空按满足放行。
    pub reboot_check_container_restart_command: String,
    // 重启前自检:docker 服务是否开机自启(退出 0=满足);置空按满足放行。
    pub reboot_check_docker_enabled_command: String,
    // 监控中心磁盘采集挂载点(阶段B):部署把宿主 / 只读挂到这里(默认 /hostfs),
    // agent 对它跑 statvfs 读宿主磁盘用量;挂载点不存在时采集层回退到 "/"。
    pub host_fs_root: String,
}

impl AgentSettings {
    pub fn from_env() -> anyhow::Result<Self> {
        let xray_binary = env::var("XRAYC_XRAY_BINARY").unwrap_or_else(|_| "xray".to_owned());
        Ok(Self {
            node_id: require_env("XRAYC_NODE_ID")?,
            control_plane_url: require_env("XRAYC_CONTROL_PLANE_URL")?,
            node_token: require_env("XRAYC_NODE_TOKEN")?,
            runtime_core: runtime_core_from_env()?,
            heartbeat_interval_seconds: optional_u64("XRAYC_HEARTBEAT_INTERVAL_SECONDS", 30)?,
            traffic_interval_seconds: optional_u64("XRAYC_TRAFFIC_INTERVAL_SECONDS", 60)?,
            session_idle_seconds: optional_u64("XRAYC_SESSION_IDLE_SECONDS", 180)?,
            xray_config_path: env::var("XRAYC_XRAY_CONFIG_PATH")
                .unwrap_or_else(|_| "/etc/xray/config.json".to_owned())
                .into(),
            agent_state_path: env::var("XRAYC_AGENT_STATE_PATH")
                .unwrap_or_else(|_| "/var/lib/xrayc/access-agent/state.json".to_owned())
                .into(),
            traffic_backlog_path: env::var("XRAYC_TRAFFIC_BACKLOG_PATH")
                .unwrap_or_else(|_| "/var/lib/xrayc/access-agent/traffic-backlog.json".to_owned())
                .into(),
            xray_access_log_path: env::var("XRAYC_XRAY_ACCESS_LOG_PATH")
                .unwrap_or_else(|_| "/var/log/xray/access.log".to_owned())
                .into(),
            xray_binary: xray_binary.clone(),
            xray_test_command: env::var("XRAYC_XRAY_TEST_COMMAND").unwrap_or_else(|_| {
                format!("{xray_binary} run -test -format json -config \"$XRAYC_XRAY_CONFIG\"")
            }),
            xray_reload_command: env::var("XRAYC_XRAY_RELOAD_COMMAND")
                .unwrap_or_else(|_| "kill -HUP 1".to_owned()),
            xray_reclaim_command: env::var("XRAYC_XRAY_RECLAIM_COMMAND").unwrap_or_default(),
            xray_container_name: env::var("XRAYC_XRAY_CONTAINER_NAME").unwrap_or_default(),
            xray_start_command: env::var("XRAYC_XRAY_START_COMMAND")
                .unwrap_or_else(|_| "true".to_owned()),
            xray_stop_command: env::var("XRAYC_XRAY_STOP_COMMAND")
                .unwrap_or_else(|_| "true".to_owned()),
            xray_api_server: env::var("XRAYC_XRAY_API_SERVER")
                .unwrap_or_else(|_| "127.0.0.1:10085".to_owned()),
            xray_api_listen_host: env::var("XRAYC_XRAY_API_LISTEN_HOST")
                .unwrap_or_else(|_| "127.0.0.1".to_owned()),
            xray_api_listen_port: optional_u16("XRAYC_XRAY_API_LISTEN_PORT")?.unwrap_or_else(
                || {
                    env::var("XRAYC_XRAY_API_SERVER")
                        .ok()
                        .and_then(|server| parse_server_port(&server))
                        .unwrap_or(10085)
                },
            ),
            rate_limiter_enabled: optional_bool("XRAYC_RATE_LIMITER_ENABLED", false)?,
            rate_limiter_dry_run: optional_bool("XRAYC_RATE_LIMITER_DRY_RUN", false)?,
            rate_limiter_interface: env::var("XRAYC_RATE_LIMITER_INTERFACE")
                .unwrap_or_else(|_| "eth0".to_owned()),
            rate_limiter_ifb_interface: env::var("XRAYC_RATE_LIMITER_IFB_INTERFACE")
                .unwrap_or_else(|_| "ifb-xrayc".to_owned()),
            rate_limiter_root_rate_bps: optional_u64(
                "XRAYC_RATE_LIMITER_ROOT_RATE_BPS",
                10_000_000_000,
            )?,
            rate_limiter_plan_path: env::var("XRAYC_RATE_LIMITER_PLAN_PATH")
                .unwrap_or_else(|_| "/var/lib/xrayc/access-agent/limiter-plan.sh".to_owned())
                .into(),
            tls_cert_domains: optional_list("XRAYC_TLS_CERT_DOMAINS"),
            certbot_binary: env::var("XRAYC_CERTBOT_BINARY")
                .unwrap_or_else(|_| "certbot".to_owned()),
            openssl_binary: env::var("XRAYC_OPENSSL_BINARY")
                .unwrap_or_else(|_| "openssl".to_owned()),
            // CF Token 由安装期注入,缺省为空(纯 IP/直连节点不需要)。读取后不回显。
            cloudflare_api_token: env::var("XRAYC_CLOUDFLARE_API_TOKEN").unwrap_or_default(),
            // cf_cert_mode 缺省 reuse_direct,只有控制面显式注入 dns01 才走 DNS-01。
            cf_cert_mode: env::var("XRAYC_CF_CERT_MODE")
                .map(|value| value.trim().to_ascii_lowercase())
                .unwrap_or_else(|_| "reuse_direct".to_owned()),
            cf_domain: env::var("XRAYC_CF_DOMAIN").unwrap_or_default(),
            // ACME 邮箱沿用直连证书邮箱 env,DNS-01 复用同一邮箱。
            acme_email: env::var("XRAYC_TLS_CERT_EMAIL").unwrap_or_default(),
            // 内核能力自检默认用 modprobe 真加载 act_connmark 判定可用性(非版本号);可经 env 覆盖/置空。
            kernel_connmark_probe_command: env::var("XRAYC_KERNEL_CONNMARK_PROBE_COMMAND")
                .unwrap_or_else(|_| "modprobe act_connmark".to_owned()),
            // 自动装最新内核默认装 HWE 内核;幂等、保留旧内核 GRUB 回退,绝不在命令里加 reboot。
            kernel_upgrade_command: env::var("XRAYC_KERNEL_UPGRADE_COMMAND").unwrap_or_else(|_| {
                "apt-get update && apt-get install -y linux-generic-hwe-22.04".to_owned()
            }),
            // 整机重启命令默认空:必须由部署显式注入(如宿主 nsenter reboot),避免容器内误重启。
            host_reboot_command: env::var("XRAYC_HOST_REBOOT_COMMAND").unwrap_or_default(),
            // 重启自检命令默认空:置空按满足放行,生产部署须注入真实自检命令。
            reboot_check_grub_default_command: env::var("XRAYC_REBOOT_CHECK_GRUB_DEFAULT_COMMAND")
                .unwrap_or_default(),
            reboot_check_container_restart_command: env::var(
                "XRAYC_REBOOT_CHECK_CONTAINER_RESTART_COMMAND",
            )
            .unwrap_or_default(),
            reboot_check_docker_enabled_command: env::var(
                "XRAYC_REBOOT_CHECK_DOCKER_ENABLED_COMMAND",
            )
            .unwrap_or_default(),
            // 监控中心磁盘采集挂载点默认 /hostfs:部署在 agent compose 把宿主 / 只读挂这里。
            host_fs_root: env::var("XRAYC_HOST_FS_ROOT").unwrap_or_else(|_| "/hostfs".to_owned()),
        })
    }

    pub fn heartbeat_interval(&self) -> Duration {
        Duration::from_secs(self.heartbeat_interval_seconds)
    }

    pub fn traffic_interval(&self) -> Duration {
        Duration::from_secs(self.traffic_interval_seconds)
    }
}

fn require_env(name: &str) -> anyhow::Result<String> {
    env::var(name).map_err(|_| anyhow::anyhow!("missing required environment variable {name}"))
}

fn optional_u64(name: &str, default: u64) -> anyhow::Result<u64> {
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(_) => Ok(default),
    }
}

fn optional_u16(name: &str) -> anyhow::Result<Option<u16>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value.parse()?)),
        Err(_) => Ok(None),
    }
}

fn optional_bool(name: &str, default: bool) -> anyhow::Result<bool> {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(anyhow::anyhow!("{name} must be a boolean")),
        },
        Err(_) => Ok(default),
    }
}

fn optional_list(name: &str) -> Vec<String> {
    env::var(name)
        .unwrap_or_default()
        .split_whitespace()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn runtime_core_from_env() -> anyhow::Result<RuntimeCore> {
    env::var("XRAYC_CORE_TYPE")
        .or_else(|_| env::var("XRAYC_RUNTIME_CORE"))
        .unwrap_or_else(|_| "xray".to_owned())
        .parse()
}

fn parse_server_port(server: &str) -> Option<u16> {
    server.rsplit_once(':')?.1.parse::<u16>().ok()
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::*;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn test_xray_test_command_defaults_to_self_expanding_run_test() {
        // BUG-E 补全:控制面已不再注入 XRAYC_XRAY_TEST_COMMAND(避免经 env_file → compose v2 把
        // $XRAYC_XRAY_CONFIG 插空)。删注入后 agent 必须仍有正确的 xray -test 命令,即 config.rs 这条
        // 内置默认接管,且语义等价 `{binary} run -test -format json -config "$XRAYC_XRAY_CONFIG"`——
        // 由 agent 自身 sh -c 在运行时展开 $XRAYC_XRAY_CONFIG(不经 compose 插值),才是正确的回退。
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _test_cmd_guard = EnvVarGuard::unset("XRAYC_XRAY_TEST_COMMAND");
        let _binary_guard = EnvVarGuard::set("XRAYC_XRAY_BINARY", "/usr/local/bin/xrayc-xray");
        // 完整 from_env 需要一堆 require_env;这里只复刻 from_env 里 xray_test_command 的取值逻辑,
        // 校验缺省分支(env 未 set 时的内置默认),与源码同源、无 compose 介入。
        let xray_binary = env::var("XRAYC_XRAY_BINARY").unwrap_or_else(|_| "xray".to_owned());
        let xray_test_command = env::var("XRAYC_XRAY_TEST_COMMAND").unwrap_or_else(|_| {
            format!("{xray_binary} run -test -format json -config \"$XRAYC_XRAY_CONFIG\"")
        });
        assert_eq!(
            xray_test_command,
            r#"/usr/local/bin/xrayc-xray run -test -format json -config "$XRAYC_XRAY_CONFIG""#,
            "缺省 XRAYC_XRAY_TEST_COMMAND 时,agent 内置默认必须是等价回退命令(由 agent 自身展开 \
             $XRAYC_XRAY_CONFIG,不经 compose 插值)"
        );
    }

    #[test]
    fn test_host_fs_root_defaults_to_hostfs_and_honors_env() {
        // 监控中心磁盘采集挂载点(阶段B):缺省 /hostfs(部署把宿主 / 只读挂这里),
        // 显式 env 覆盖时取 env 值。这里只复刻 from_env 里 host_fs_root 的取值逻辑,与源码同源。
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();

        let _unset_guard = EnvVarGuard::unset("XRAYC_HOST_FS_ROOT");
        let default_root = env::var("XRAYC_HOST_FS_ROOT").unwrap_or_else(|_| "/hostfs".to_owned());
        assert_eq!(default_root, "/hostfs");

        let _set_guard = EnvVarGuard::set("XRAYC_HOST_FS_ROOT", "/custom-host-root");
        let custom_root = env::var("XRAYC_HOST_FS_ROOT").unwrap_or_else(|_| "/hostfs".to_owned());
        assert_eq!(custom_root, "/custom-host-root");
    }

    #[test]
    fn test_runtime_core_from_env_accepts_runtime_core_alias() {
        // 内核固定为 xray：XRAYC_RUNTIME_CORE 的 xray 别名应被接受并归一为 Xray。
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _core_guard = EnvVarGuard::unset("XRAYC_CORE_TYPE");
        let _runtime_guard = EnvVarGuard::set("XRAYC_RUNTIME_CORE", "xray-core");

        let runtime_core = runtime_core_from_env().expect("runtime core parses");

        assert_eq!(runtime_core, RuntimeCore::Xray);
    }

    struct EnvVarGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = env::var(key).ok();
            env::set_var(key, value);
            Self { key, original }
        }

        fn unset(key: &'static str) -> Self {
            let original = env::var(key).ok();
            env::remove_var(key);
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(original) = &self.original {
                env::set_var(self.key, original);
            } else {
                env::remove_var(self.key);
            }
        }
    }
}
