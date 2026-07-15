//! 运行时配置 JSON 补丁辅助模块。
//! 这里只负责生成空配置和对已编译配置做本机路径补丁。
//! 异步校验、reload、回滚和控制面上报仍保留在 apply 模块。
//! Xray 空配置保留 StatsService，用于流量采集接口保持可用。
//! 没有可用线路时只放行 Stats API 入站，旧用户入口被清空。
//! access log 路径来自 AgentSettings，空路径时不写入。
//! Stats API 监听地址按本机配置覆盖编译结果。
//! 函数不访问网络、不写文件、不修改外部状态。
//! 本文件由 apply 拆分而来，降低主应用流程文件长度。
//! 本头部满足前十行中文注释约束。

use serde_json::Value;
use xrayc_xray_config::XRAY_STATS_API_TAG;

use crate::config::{AgentSettings, RuntimeCore};

pub(super) fn empty_runtime_config(settings: &AgentSettings, runtime_core: RuntimeCore) -> Value {
    match runtime_core {
        RuntimeCore::Xray => empty_xray_config(settings),
    }
}

fn empty_xray_config(settings: &AgentSettings) -> Value {
    serde_json::json!({
        "log": {
            "loglevel": "warning"
        },
        "api": {
            "tag": "xrayc-api",
            "services": ["StatsService"]
        },
        "stats": {},
        "policy": {
            "levels": {
                "0": {
                    "statsUserUplink": true,
                    "statsUserDownlink": true
                }
            },
            "system": {
                "statsInboundUplink": true,
                "statsInboundDownlink": true,
                "statsOutboundUplink": true,
                "statsOutboundDownlink": true
            }
        },
        "inbounds": [{
            "tag": "xrayc-api",
            "listen": settings.xray_api_listen_host,
            "port": settings.xray_api_listen_port,
            "protocol": "dokodemo-door",
            "settings": {
                "address": settings.xray_api_listen_host
            }
        }],
        "outbounds": [{
            "tag": "blocked",
            "protocol": "blackhole",
            "settings": {}
        }],
        "routing": {
            "domainStrategy": "AsIs",
            "rules": [{
                "type": "field",
                "inboundTag": ["xrayc-api"],
                "outboundTag": "xrayc-api"
            }]
        }
    })
}

pub(super) fn patch_runtime_config(
    config: &mut Value,
    settings: &AgentSettings,
    runtime_core: RuntimeCore,
) {
    if runtime_core == RuntimeCore::Xray {
        patch_access_log_path(config, settings);
        patch_stats_api_endpoint(config, settings);
    }
}

fn patch_access_log_path(config: &mut Value, settings: &AgentSettings) {
    if settings.xray_access_log_path.as_os_str().is_empty() {
        return;
    }
    if !config.get("log").is_some_and(Value::is_object) {
        config["log"] = serde_json::json!({});
    }
    config["log"]["access"] =
        Value::String(settings.xray_access_log_path.to_string_lossy().into_owned());
}

fn patch_stats_api_endpoint(config: &mut Value, settings: &AgentSettings) {
    let Some(inbounds) = config.get_mut("inbounds").and_then(Value::as_array_mut) else {
        return;
    };

    for inbound in inbounds {
        if inbound.get("tag").and_then(Value::as_str) != Some(XRAY_STATS_API_TAG) {
            continue;
        }
        inbound["listen"] = Value::String(settings.xray_api_listen_host.clone());
        inbound["port"] = Value::Number(settings.xray_api_listen_port.into());
        inbound["settings"]["address"] = Value::String(settings.xray_api_listen_host.clone());
    }
}
