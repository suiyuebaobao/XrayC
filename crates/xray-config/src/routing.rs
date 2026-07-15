//! 本模块负责把访问控制路由规则转换为 Xray 路由规则。
//! 路由规则按入口标签和用户邮箱匹配，再指向指定出口标签。
//! 统计接口路由规则由统计模块单独维护。

use serde_json::{json, Map, Value};

use crate::RoutingRule;

pub(crate) fn compile_routing_rule(rule: &RoutingRule) -> Value {
    let mut object = Map::new();
    object.insert("type".to_owned(), json!("field"));
    object.insert("outboundTag".to_owned(), json!(rule.outbound_tag));

    if let Some(inbound_tag) = &rule.inbound_tag {
        object.insert("inboundTag".to_owned(), json!([inbound_tag]));
    }
    if let Some(email) = &rule.user_email {
        object.insert("user".to_owned(), json!([email]));
    }

    Value::Object(object)
}
