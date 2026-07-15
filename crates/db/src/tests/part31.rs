// 数据库测试分片 31。
// 本文件覆盖严格用户级限速的共享入站 + 每用户 fwmark 渲染。
// 分片只用于满足单文件行数限制,不改变测试作用域。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 测试只使用内存 Store,不访问真实服务器或数据库。
// 限速/不限速用户都进同线路共享入站(守真实端口),凭据区分。
// 限速差异只体现在带 sockopt.mark 的出站 + user 路由规则。
// Agent 上报仍必须使用原始 source_line_id。
// 这里不保存真实 token、域名、账号或外部服务资料。
// 本头部满足前十行中文注释约束。

// 用户级限速 fwmark 区间:与 core 实现一致,测试仅断言"落区间"不固定具体哈希值。
const RATE_LIMIT_MARK_START: u64 = 0x1_0000;
const RATE_LIMIT_MARK_END: u64 = 0xF_FFFF;

#[test]
fn test_heartbeat_rate_limited_users_share_inbound_with_distinct_marks() {
    let store = MemoryStore::seeded();
    let node_id = store.read(|data| *data.access_nodes.keys().next().unwrap());
    let (base_line_id, base_port) = store.read(|data| {
        let line = data.access_lines.values().next().unwrap();
        (line.id, line.listen_port)
    });
    store.write(|data| {
        let line = data.access_lines[&base_line_id].clone();
        let endpoint = data
            .exit_pools
            .get(&line.exit_pool_id)
            .unwrap()
            .members
            .first()
            .unwrap()
            .clone();
        let plan_id = data.subscriptions.values().next().unwrap().plan_id;
        data.plans.get_mut(&plan_id).unwrap().rate_limit_bps = 1_000_000;
        let first_user_id = data.tokens["demo-token"].user_id;
        let second_user_id = Uuid::new_v4();
        data.users.insert(
            second_user_id,
            User {
                id: second_user_id,
                email: "second@example.test".to_string(),
                xray_user_key: format!("u-{}@xrayc.local", second_user_id.simple()),
                access_credential: Uuid::new_v4().to_string(),
                disabled: false,
                rate_limit_bps: None,
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
            },
        );
        data.subscriptions.insert(
            second_user_id,
            UserSubscription {
                user_id: second_user_id,
                plan_id,
                active: true,
                expires_at: Utc::now() + chrono::Duration::days(30),
                used_bytes: 0,
                limit_bytes: 10 * 1024 * 1024 * 1024,
            },
        );
        data.tokens.insert(
            "fixture-visible-subscription".to_string(),
            SubscriptionToken {
                token: String::new(),
                user_id: second_user_id,
            },
        );
        for user_id in [first_user_id, second_user_id] {
            data.user_exit_assignments.push(UserExitAssignment {
                user_id,
                access_line_id: line.id,
                exit_pool_id: line.exit_pool_id,
                exit_endpoint_id: endpoint.id,
                assigned_at: Utc::now(),
                failover_reason: "unit-test".to_string(),
            });
        }
    });

    let heartbeat = store.read(|data| heartbeat_json(data, Some(node_id), None));
    let config = &heartbeat["config"];
    let access_lines = config["access_lines"].as_array().unwrap();

    // 两个限速用户共享同一条真实端口入站(单入站多用户),不再按用户拆运行时端口。
    assert_eq!(access_lines.len(), 1);
    assert_eq!(access_lines[0]["listen_port"], u64::from(base_port));
    assert_eq!(access_lines[0]["source_line_id"], base_line_id.to_string());
    assert_eq!(access_lines[0]["id"], base_line_id.to_string());
    assert_eq!(access_lines[0]["users"].as_array().unwrap().len(), 2);

    // 两个限速用户各分一个落区间且互不相撞的 fwmark。
    let rate_limits = config["rate_limits"].as_array().unwrap();
    assert_eq!(rate_limits.len(), 2);
    let mut marks = rate_limits
        .iter()
        .map(|entry| entry["mark"].as_u64().unwrap())
        .collect::<Vec<_>>();
    for mark in &marks {
        assert!(
            (RATE_LIMIT_MARK_START..=RATE_LIMIT_MARK_END).contains(mark),
            "mark 必须落区间 [0x10000,0xFFFFF],实际 {mark:#x}"
        );
    }
    marks.sort_unstable();
    marks.dedup();
    assert_eq!(marks.len(), 2, "同节点不同限速用户的 mark 不得相撞");

    // 两条带 mark 的出站 + 两条 user 路由规则。
    let marked_endpoints = config["exit_endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|endpoint| endpoint["tag"].as_str().is_some_and(|tag| tag.contains("-user-")))
        .count();
    assert_eq!(marked_endpoints, 2);
    let marked_rules = config["routing_rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|rule| rule["outbound_tag"].as_str().is_some_and(|tag| tag.contains("-user-")))
        .count();
    assert_eq!(marked_rules, 2);
}

#[test]
fn test_heartbeat_mixed_rate_limited_and_unlimited_users_share_single_inbound() {
    let store = MemoryStore::seeded();
    let node_id = store.read(|data| *data.access_nodes.keys().next().unwrap());
    let (base_line_id, base_port) = store.read(|data| {
        let line = data.access_lines.values().next().unwrap();
        (line.id, line.listen_port)
    });
    store.write(|data| {
        let line = data.access_lines[&base_line_id].clone();
        let endpoint = data
            .exit_pools
            .get(&line.exit_pool_id)
            .unwrap()
            .members
            .first()
            .unwrap()
            .clone();
        let plan_id = data.subscriptions.values().next().unwrap().plan_id;
        // 套餐保持不限速,只单独给第二用户开限速 → 一限速一不限速。
        let first_user_id = data.tokens["demo-token"].user_id;
        let second_user_id = Uuid::new_v4();
        data.users.insert(
            second_user_id,
            User {
                id: second_user_id,
                email: "limited@example.test".to_string(),
                xray_user_key: format!("u-{}@xrayc.local", second_user_id.simple()),
                access_credential: Uuid::new_v4().to_string(),
                disabled: false,
                rate_limit_bps: Some(5_000_000),
                rate_limit_up_bps: None,
                rate_limit_down_bps: None,
            },
        );
        data.subscriptions.insert(
            second_user_id,
            UserSubscription {
                user_id: second_user_id,
                plan_id,
                active: true,
                expires_at: Utc::now() + chrono::Duration::days(30),
                used_bytes: 0,
                limit_bytes: 10 * 1024 * 1024 * 1024,
            },
        );
        data.tokens.insert(
            "fixture-visible-subscription".to_string(),
            SubscriptionToken {
                token: String::new(),
                user_id: second_user_id,
            },
        );
        for user_id in [first_user_id, second_user_id] {
            data.user_exit_assignments.push(UserExitAssignment {
                user_id,
                access_line_id: line.id,
                exit_pool_id: line.exit_pool_id,
                exit_endpoint_id: endpoint.id,
                assigned_at: Utc::now(),
                failover_reason: "unit-test".to_string(),
            });
        }
    });

    let heartbeat = store.read(|data| heartbeat_json(data, Some(node_id), None));
    let config = &heartbeat["config"];
    let access_lines = config["access_lines"].as_array().unwrap();

    // 只渲染 1 条真实端口入站,含两个用户(限速 + 不限速同入站)。
    assert_eq!(access_lines.len(), 1);
    assert_eq!(access_lines[0]["listen_port"], u64::from(base_port));
    assert_eq!(access_lines[0]["users"].as_array().unwrap().len(), 2);

    // 仅限速用户进 rate_limits,且 mark 落区间。
    let rate_limits = config["rate_limits"].as_array().unwrap();
    assert_eq!(rate_limits.len(), 1);
    let mark = rate_limits[0]["mark"].as_u64().unwrap();
    assert!(
        (RATE_LIMIT_MARK_START..=RATE_LIMIT_MARK_END).contains(&mark),
        "限速用户 mark 必须落区间 [0x10000,0xFFFFF],实际 {mark:#x}"
    );

    // 限速用户有带 mark 出站 + user 路由规则;不限速用户走共享出口(无 mark)。
    let marked_endpoints = config["exit_endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|endpoint| endpoint["tag"].as_str().is_some_and(|tag| tag.contains("-user-")))
        .count();
    assert_eq!(marked_endpoints, 1);
    let routing_rules = config["routing_rules"].as_array().unwrap();
    let marked_rules = routing_rules
        .iter()
        .filter(|rule| rule["outbound_tag"].as_str().is_some_and(|tag| tag.contains("-user-")))
        .count();
    let plain_rules = routing_rules
        .iter()
        .filter(|rule| {
            rule["outbound_tag"]
                .as_str()
                .is_some_and(|tag| !tag.contains("-user-"))
        })
        .count();
    assert_eq!(marked_rules, 1, "限速用户应有一条带 mark 出站的 user 路由");
    assert!(plain_rules >= 1, "不限速用户应有一条走共享出口的 user 路由");
}

#[test]
fn test_subscription_read_model_uses_real_port_for_limited_user() {
    let store = MemoryStore::seeded();
    let (user_id, base_port) = store.read(|data| {
        let user_id = data.tokens["demo-token"].user_id;
        let base_port = data.access_lines.values().next().unwrap().listen_port;
        (user_id, base_port)
    });
    store.write(|data| {
        let line = data.access_lines.values().next().unwrap().clone();
        let endpoint = data
            .exit_pools
            .get(&line.exit_pool_id)
            .unwrap()
            .members
            .first()
            .unwrap()
            .clone();
        let plan_id = data.subscriptions[&user_id].plan_id;
        data.plans.get_mut(&plan_id).unwrap().rate_limit_bps = 1_000_000;
        data.user_exit_assignments.push(UserExitAssignment {
            user_id,
            access_line_id: line.id,
            exit_pool_id: line.exit_pool_id,
            exit_endpoint_id: endpoint.id,
            assigned_at: Utc::now(),
            failover_reason: "unit-test".to_string(),
        });
    });

    let subscription = store.read(|data| user_subscription_json_for_user(data, user_id));
    let access_lines = subscription["access_lines"].as_array().unwrap();

    // 限速已改用 fwmark,订阅入口端口必须仍是线路真实监听端口。
    assert_eq!(access_lines.len(), 1);
    assert_eq!(access_lines[0]["listen_port"], u64::from(base_port));
}

#[test]
fn test_heartbeat_rate_limited_vless_xudp_exit_gets_marked_outbound() {
    let store = MemoryStore::seeded();
    let node_id = store.read(|data| *data.access_nodes.keys().next().unwrap());
    store.write(|data| {
        let user_id = data.tokens["demo-token"].user_id;
        let line = data.access_lines.values().next().unwrap().clone();
        let pool = data.exit_pools.get_mut(&line.exit_pool_id).unwrap();
        let endpoint = pool.members.first_mut().unwrap();
        endpoint.outbound_type = EndpointType::Vless;
        endpoint.outbound_config = json!({
            "uuid": "00000000-0000-4000-8000-000000000031",
            "security": "none"
        });
        endpoint.stream_config = json!({
            "network_mode": "xudp",
            "udp_packet_encoding": "xudp"
        });
        let plan_id = data.subscriptions[&user_id].plan_id;
        data.plans.get_mut(&plan_id).unwrap().rate_limit_bps = 1_000_000;
        data.users.get_mut(&user_id).unwrap().rate_limit_bps = Some(4_000_000);
        data.user_exit_assignments.push(UserExitAssignment {
            user_id,
            access_line_id: line.id,
            exit_pool_id: line.exit_pool_id,
            exit_endpoint_id: endpoint.id,
            assigned_at: Utc::now(),
            failover_reason: "unit-test".to_string(),
        });
    });

    let heartbeat = store.read(|data| heartbeat_json(data, Some(node_id), None));
    let config = &heartbeat["config"];
    let mark = config["rate_limits"][0]["mark"].as_u64().unwrap();
    let marked_endpoint = config["exit_endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|endpoint| endpoint["tag"].as_str().is_some_and(|tag| tag.contains("-user-")))
        .expect("marked endpoint exists");

    assert_eq!(config["rate_limits"][0]["rate_limit_bps"], 4_000_000);
    assert_eq!(marked_endpoint["sockopt_mark"], mark);
    assert_eq!(marked_endpoint["protocol"]["type"], "vless");
}

#[test]
fn test_subscription_read_model_keeps_groups_that_share_same_line() {
    let store = MemoryStore::seeded();
    let user_id = store.read(|data| data.tokens["demo-token"].user_id);
    store.write(|data| {
        let first_group_id = data.plans.values().next().unwrap().line_groups[0].line_group_id;
        let endpoint_id = data.line_groups[&first_group_id].line_ids[0];
        let second_group_id = Uuid::new_v4();
        data.line_groups.insert(
            second_group_id,
            LineGroup {
                id: second_group_id,
                name: "GPT 分组".to_string(),
                country_code: "US".to_string(),
                icon: String::new(),
                sort_weight: 200,
                billing_multiplier: 1.0,
                enabled: true,
                exit_pool_id: data.line_groups[&first_group_id].exit_pool_id,
                line_ids: vec![endpoint_id],
                binding_node_ids: Vec::new(),
                dedicated_rules: Vec::new(),
                rule_set_bindings: Vec::new(),
            },
        );
        let plan = data.plans.values_mut().next().unwrap();
        plan.line_group_ids = vec![first_group_id, second_group_id];
        plan.line_groups = vec![
            PlanLineGroup {
                line_group_id: first_group_id,
                billing_multiplier: 1.0,
            },
            PlanLineGroup {
                line_group_id: second_group_id,
                billing_multiplier: 1.0,
            },
        ];
    });

    let subscription = store.read(|data| user_subscription_json_for_user(data, user_id));
    let access_lines = subscription["access_lines"].as_array().unwrap();
    let group_names = access_lines
        .iter()
        .map(|line| line["line_group_name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();

    assert_eq!(access_lines.len(), 2);
    assert!(group_names.contains(&"默认分组"));
    assert!(group_names.contains(&"GPT 分组"));
}

#[test]
fn test_heartbeat_rate_limited_self_hosted_local_exit_marked_outbound_folds_to_direct() {
    // 回归:限速用户路由到「本机出口(self_hosted、本节点自有)」时,本机出口本质是本节点
    // freedom 直连出网(xray-config 给每个本机出口服务硬编码 freedom 出站)。若仍按协议把带
    // mark 出站渲染成「连本机出口环回入站(127.0.0.1:<port>)」的 socks,这一跳走 lo 不经 eth0,
    // eth0/ifb 限速器抓不到 → 限速失效(真机铁证)。修复:折叠成带该用户 mark 的 freedom(Direct)
    // 直连出站,让 用户→公网 的包带 mark 直接走 eth0,限速器抓得到。
    let store = MemoryStore::seeded();
    let node_id = store.read(|data| *data.access_nodes.keys().next().unwrap());
    store.write(|data| {
        let user_id = data.tokens["demo-token"].user_id;
        let line = data.access_lines.values().next().unwrap().clone();
        let pool = data.exit_pools.get_mut(&line.exit_pool_id).unwrap();
        let endpoint = pool.members.first_mut().unwrap();
        // 把种子出口改成「本节点自有的本机 SOCKS 出口」:中转直接连它走环回 127.0.0.1。
        endpoint.ownership = "self_hosted".to_string();
        endpoint.owner_access_node_id = Some(node_id);
        endpoint.outbound_type = EndpointType::Socks;
        endpoint.host = "127.0.0.1".to_string();
        endpoint.port = 21080;
        endpoint.outbound_config = json!({
            "username": "local-exit-user",
            "password": "local-exit-pass"
        });
        let plan_id = data.subscriptions[&user_id].plan_id;
        data.plans.get_mut(&plan_id).unwrap().rate_limit_bps = 1_000_000;
        data.users.get_mut(&user_id).unwrap().rate_limit_bps = Some(3_000_000);
        data.user_exit_assignments.push(UserExitAssignment {
            user_id,
            access_line_id: line.id,
            exit_pool_id: line.exit_pool_id,
            exit_endpoint_id: endpoint.id,
            assigned_at: Utc::now(),
            failover_reason: "unit-test".to_string(),
        });
    });

    let heartbeat = store.read(|data| heartbeat_json(data, Some(node_id), None));
    let config = &heartbeat["config"];
    let mark = config["rate_limits"][0]["mark"].as_u64().unwrap();
    let marked_endpoint = config["exit_endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|endpoint| endpoint["tag"].as_str().is_some_and(|tag| tag.contains("-user-")))
        .expect("marked endpoint exists");

    // 关键:带 mark 出站必须是 freedom(direct)直连出公网,而不是连本机出口环回入站的 socks;
    // 唯有 freedom 直连才让 mark 落在 eth0 出网包上,被 agent 的 eth0/ifb 限速器抓到。
    assert_eq!(
        marked_endpoint["protocol"]["type"], "direct",
        "本节点自有本机出口的带 mark 出站应折叠成 freedom 直连,让 mark 落 eth0 被限速器抓到"
    );
    assert_eq!(
        marked_endpoint["sockopt_mark"], mark,
        "折叠后的 freedom 出站仍须带该用户 fwmark"
    );
}

#[test]
fn test_heartbeat_rate_limited_cross_node_self_hosted_exit_not_folded() {
    // 守护谓词不过度折叠:self_hosted 出口若 owner 是「别的节点」,中转连它是经 eth0 的真实网络跳
    // (不是本机环回),mark 本就落 eth0、限速器抓得到。此时绝不能折叠成本节点 freedom,否则会把出网
    // IP 从对端节点错改成本节点。断言带 mark 出站仍保持其原协议(socks),不被折叠成 direct。
    let store = MemoryStore::seeded();
    let node_id = store.read(|data| *data.access_nodes.keys().next().unwrap());
    let other_node_id = Uuid::new_v4();
    store.write(|data| {
        let user_id = data.tokens["demo-token"].user_id;
        let line = data.access_lines.values().next().unwrap().clone();
        let pool = data.exit_pools.get_mut(&line.exit_pool_id).unwrap();
        let endpoint = pool.members.first_mut().unwrap();
        // self_hosted 但归属另一节点:本节点连它走 eth0,非环回。
        endpoint.ownership = "self_hosted".to_string();
        endpoint.owner_access_node_id = Some(other_node_id);
        endpoint.outbound_type = EndpointType::Socks;
        endpoint.host = "198.51.100.20".to_string();
        endpoint.port = 1080;
        endpoint.outbound_config = json!({});
        let plan_id = data.subscriptions[&user_id].plan_id;
        data.plans.get_mut(&plan_id).unwrap().rate_limit_bps = 1_000_000;
        data.users.get_mut(&user_id).unwrap().rate_limit_bps = Some(3_000_000);
        data.user_exit_assignments.push(UserExitAssignment {
            user_id,
            access_line_id: line.id,
            exit_pool_id: line.exit_pool_id,
            exit_endpoint_id: endpoint.id,
            assigned_at: Utc::now(),
            failover_reason: "unit-test".to_string(),
        });
    });

    let heartbeat = store.read(|data| heartbeat_json(data, Some(node_id), None));
    let config = &heartbeat["config"];
    let marked_endpoint = config["exit_endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|endpoint| endpoint["tag"].as_str().is_some_and(|tag| tag.contains("-user-")))
        .expect("marked endpoint exists");
    assert_eq!(
        marked_endpoint["protocol"]["type"], "socks",
        "跨节点 self_hosted 是经 eth0 的真实网络跳,带 mark 出站不能折叠成本节点 freedom"
    );
}

#[test]
fn test_heartbeat_unlimited_self_hosted_user_keeps_loopback_outbound_and_service() {
    // 守护计费/统计归属:不限速用户路由到本节点本机出口时,出站不带 mark、仍走「连环回本机出口入站」的
    // 原协议(socks),本机出口服务入站照常渲染。修复只折叠「带 mark」出站,绝不动不限速路径,
    // 故本机出口服务的入站统计与既有行为零变化。
    let store = MemoryStore::seeded();
    let node_id = store.read(|data| *data.access_nodes.keys().next().unwrap());
    store.write(|data| {
        let user_id = data.tokens["demo-token"].user_id;
        let line = data.access_lines.values().next().unwrap().clone();
        let pool = data.exit_pools.get_mut(&line.exit_pool_id).unwrap();
        let endpoint = pool.members.first_mut().unwrap();
        endpoint.ownership = "self_hosted".to_string();
        endpoint.owner_access_node_id = Some(node_id);
        endpoint.outbound_type = EndpointType::Socks;
        endpoint.host = "127.0.0.1".to_string();
        endpoint.port = 21080;
        endpoint.outbound_config = json!({
            "username": "local-exit-user",
            "password": "local-exit-pass"
        });
        // 忠实数据模型:本机出口同时在 local_exit_endpoints(渲染服务入站)与 exit_pools(供绑定/出站)。
        let local_exit = endpoint.clone();
        data.local_exit_endpoints.push(local_exit);
        // 套餐与用户都不限速 → 不分配 mark。
        data.user_exit_assignments.push(UserExitAssignment {
            user_id,
            access_line_id: line.id,
            exit_pool_id: line.exit_pool_id,
            exit_endpoint_id: endpoint.id,
            assigned_at: Utc::now(),
            failover_reason: "unit-test".to_string(),
        });
    });

    let heartbeat = store.read(|data| heartbeat_json(data, Some(node_id), None));
    let config = &heartbeat["config"];

    // 不限速用户:无 rate_limits、无带 mark 出站。
    assert!(config["rate_limits"].as_array().unwrap().is_empty());
    let marked = config["exit_endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|endpoint| endpoint["tag"].as_str().is_some_and(|tag| tag.contains("-user-")))
        .count();
    assert_eq!(marked, 0, "不限速用户不得有带 mark 出站");

    // 不限速出口出站仍是连环回本机出口入站的原协议(socks),不被折叠成 direct。
    let plain_endpoint = config["exit_endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|endpoint| {
            endpoint["tag"]
                .as_str()
                .is_some_and(|tag| tag.starts_with("exit-") && !tag.contains("-user-"))
        })
        .expect("unmarked exit endpoint exists");
    assert_eq!(plain_endpoint["protocol"]["type"], "socks");

    // 本机出口服务入站照常渲染(统计归属载体保留)。
    assert!(
        !config["local_exit_services"].as_array().unwrap().is_empty(),
        "本机出口服务入站应照常渲染,保留其入站统计归属"
    );
}
