//! 本模块实现 Xray 配置编译器入口。
//! 具体入站、出站、路由、统计和类型定义拆分到同目录子模块。
//! 这里保留包的对外公开接口，并组装最终核心配置。

mod error;
mod inbound;
mod outbound;
mod routing;
mod stats;
#[cfg(test)]
mod tests;
mod types;
mod util;

use serde_json::{json, Value};

use inbound::{compile_inbound, compile_local_exit_service_inbound};
use outbound::compile_outbound;
use routing::compile_routing_rule;
use stats::{compile_stats_api_inbound, compile_stats_api_routing_rule};

pub use error::XrayConfigError;
pub use stats::{
    parse_stats_user_email, stats_user_email, XRAY_STATS_API_LISTEN, XRAY_STATS_API_PORT,
    XRAY_STATS_API_TAG,
};
pub use types::{
    AccessConfig, AccessLine, AccessProtocol, AccessUser, ExitEndpoint, ExitProtocol,
    LocalExitProtocol, LocalExitService, LogLevel, RoutingRule, UserRateLimit,
};

pub fn compile_xray_config(config: &AccessConfig) -> Result<Value, XrayConfigError> {
    let mut inbounds = config
        .access_lines
        .iter()
        .map(compile_inbound)
        .collect::<Result<Vec<_>, _>>()?;
    inbounds.extend(
        config
            .local_exit_services
            .iter()
            .map(compile_local_exit_service_inbound)
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut outbounds = config
        .exit_endpoints
        .iter()
        .map(compile_outbound)
        .collect::<Result<Vec<_>, _>>()?;
    let mut routing_rules = config
        .routing_rules
        .iter()
        .map(compile_routing_rule)
        .collect::<Vec<_>>();
    for service in &config.local_exit_services {
        let direct_tag = format!("{}-direct", service.tag);
        outbounds.push(json!({
            "tag": direct_tag,
            "protocol": "freedom",
            "settings": {},
        }));
        routing_rules.push(json!({
            "type": "field",
            "inboundTag": [service.tag],
            "outboundTag": direct_tag,
        }));
    }

    if config.stats_enabled {
        inbounds.push(compile_stats_api_inbound());
        routing_rules.insert(0, compile_stats_api_routing_rule());
    }

    let mut value = json!({
        "log": {
            "loglevel": config.log_level,
        },
        "api": if config.stats_enabled { json!({
            "tag": XRAY_STATS_API_TAG,
            "services": ["StatsService"],
        }) } else { Value::Null },
        "stats": if config.stats_enabled { json!({}) } else { Value::Null },
        "policy": {
            "levels": {
                "0": {
                    "statsUserUplink": true,
                    "statsUserDownlink": true,
                }
            },
            "system": {
                "statsInboundUplink": true,
                "statsInboundDownlink": true,
                "statsOutboundUplink": true,
                "statsOutboundDownlink": true,
            }
        },
        "inbounds": inbounds,
        "outbounds": outbounds,
        "routing": {
            "domainStrategy": "AsIs",
            "rules": routing_rules,
        },
    });
    if !config.stats_enabled {
        if let Some(object) = value.as_object_mut() {
            object.remove("api");
            object.remove("stats");
        }
    }
    Ok(value)
}
