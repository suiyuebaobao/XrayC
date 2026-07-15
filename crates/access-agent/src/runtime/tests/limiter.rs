//! 本模块测试 access-agent 用户级限速命令计划。
//! 新方案按「每用户 fwmark」整形(不再按入口端口区分用户):上行(节点→出口)在
//! 物理网卡 eth0 egress 上按 mark 直接整形,下行(出口→节点)经 eth0 ingress +
//! act_connmark 还原 mark 后 redirect 进 IFB 按 mark 整形。
//! 测试只验证命令生成,不执行 tc/iptables,避免单元测试修改本机网络;真实执行由真机 E2E 覆盖。

use xrayc_xray_config::UserRateLimit;

use super::super::limiter::{
    build_limiter_command_plan, shaping_present_in_qdisc_output, LimiterPlanSettings,
};

/// 核心:限速按每用户 fwmark 整形,绝不按入口端口(dport/sport)区分用户。
/// 两个限速用户(各自 mark/上行/下行),断言:
/// - 上行在 eth0 egress:每用户 `handle <mark> fw flowid 1:<class>`、class 速率=上行速率;
/// - 下行在 IFB:每用户 `handle <mark> fw flowid 2:<class>`、class 速率=下行速率;
/// - eth0 ingress 用 matchall+connmark 把回程整体还原 mark 并 redirect 进 IFB;
/// - 出站 SO_MARK 经 OUTPUT CONNMARK --save-mark 存进 conntrack(供下行回程还原);
/// - 不含任何 `match ip dport` / `match ip sport` / `skbedit mark`(端口匹配已彻底废弃)。
#[test]
fn test_limiter_shapes_by_fwmark_not_port() {
    let settings = LimiterPlanSettings {
        enabled: true,
        interface: "eth0".to_string(),
        ifb_interface: "ifb-xrayc".to_string(),
        root_rate_bps: 10_000_000_000,
        connmark_available: true,
    };
    let users = vec![
        UserRateLimit {
            user_id: "user-a".to_string(),
            xray_user_key: "u-a@xrayc.local".to_string(),
            rate_limit_bps: 10_000_000,
            rate_limit_up_bps: 2_000_000,    // 上行 2mbit
            rate_limit_down_bps: 10_000_000, // 下行 10mbit
            mark: 0x10001,
            class_id: 101,
        },
        UserRateLimit {
            user_id: "user-b".to_string(),
            xray_user_key: "u-b@xrayc.local".to_string(),
            rate_limit_bps: 30_000_000,
            rate_limit_up_bps: 5_000_000,    // 上行 5mbit
            rate_limit_down_bps: 30_000_000, // 下行 30mbit
            mark: 0x10002,
            class_id: 102,
        },
    ];

    let joined = build_limiter_command_plan(&settings, &users).join("\n");

    // 用户 A:上行(eth0 egress)按 mark 进上行 class,速率=上行速率。
    assert!(
        joined.contains(
            "tc class replace dev eth0 parent 1: classid 1:101 htb rate 2mbit ceil 2mbit"
        ),
        "用户 A 上行类应在 eth0 用上行速率 2mbit\n{joined}"
    );
    assert!(
        joined.contains(
            "tc filter replace dev eth0 parent 1: protocol ip prio 101 handle 65537 fw flowid 1:101"
        ),
        "用户 A 上行 filter 应按 fwmark 65537 进 eth0 上行 class\n{joined}"
    );
    // 用户 A:下行(IFB)按 mark 进下行 class,速率=下行速率。
    assert!(
        joined.contains(
            "tc class replace dev ifb-xrayc parent 2: classid 2:101 htb rate 10mbit ceil 10mbit"
        ),
        "用户 A 下行类应在 IFB 用下行速率 10mbit\n{joined}"
    );
    assert!(
        joined.contains(
            "tc filter replace dev ifb-xrayc parent 2: protocol ip prio 101 handle 65537 fw flowid 2:101"
        ),
        "用户 A 下行 filter 应按 fwmark 65537 进 IFB 下行 class\n{joined}"
    );
    // 用户 B 同理。
    assert!(joined
        .contains("tc class replace dev eth0 parent 1: classid 1:102 htb rate 5mbit ceil 5mbit"));
    assert!(joined.contains(
        "tc filter replace dev eth0 parent 1: protocol ip prio 102 handle 65538 fw flowid 1:102"
    ));
    assert!(joined.contains(
        "tc class replace dev ifb-xrayc parent 2: classid 2:102 htb rate 30mbit ceil 30mbit"
    ));
    assert!(joined.contains(
        "tc filter replace dev ifb-xrayc parent 2: protocol ip prio 102 handle 65538 fw flowid 2:102"
    ));

    // 下行回程整体闭环:eth0 ingress 用 matchall+connmark 还原 mark 后 redirect 进 IFB。
    assert!(joined.contains(
        "tc filter add dev eth0 parent ffff: protocol ip prio 32766 matchall action connmark action mirred egress redirect dev ifb-xrayc"
    ));
    // 出站 SO_MARK 存进 conntrack:逐用户 mark 覆盖。
    assert!(joined.contains(
        "iptables -t mangle -A XRAYC_LIMITER_OUTPUT -m mark --mark 65537 -j CONNMARK --save-mark"
    ));
    assert!(joined.contains(
        "iptables -t mangle -A XRAYC_LIMITER_OUTPUT -m mark --mark 65538 -j CONNMARK --save-mark"
    ));

    // 红线:绝不按端口区分用户,不得出现任何 dport/sport/skbedit 匹配。
    assert!(!joined.contains("match ip dport"), "不得按 dport 匹配用户");
    assert!(!joined.contains("match ip sport"), "不得按 sport 匹配用户");
    assert!(
        !joined.contains("skbedit mark"),
        "不得用 skbedit 按端口打标"
    );
}

#[test]
fn test_limiter_command_plan_builds_per_user_shared_tcp_udp_htb_rules() {
    let settings = LimiterPlanSettings {
        enabled: true,
        interface: "eth0".to_string(),
        ifb_interface: "ifb-xrayc".to_string(),
        root_rate_bps: 10_000_000_000,
        connmark_available: true,
    };
    let users = vec![
        UserRateLimit {
            user_id: "user-a".to_string(),
            xray_user_key: "u-a@xrayc.local".to_string(),
            rate_limit_bps: 25_000_000,
            rate_limit_up_bps: 25_000_000,
            rate_limit_down_bps: 25_000_000,
            mark: 65_537,
            class_id: 101,
        },
        UserRateLimit {
            user_id: "user-b".to_string(),
            xray_user_key: "u-b@xrayc.local".to_string(),
            rate_limit_bps: 50_000_000,
            rate_limit_up_bps: 50_000_000,
            rate_limit_down_bps: 50_000_000,
            mark: 65_538,
            class_id: 102,
        },
    ];

    let commands = build_limiter_command_plan(&settings, &users);
    let joined = commands.join("\n");

    assert!(joined.contains(
        "# xrayc limiter: each user rate_limit_bps is shared by marked TCP and UDP traffic"
    ));
    assert!(joined.contains("tc qdisc replace dev eth0 root handle 1: htb default 1"));
    // 单一 mark catch 该用户全部 TCP+UDP:一个用户一条 fw filter,绝不按协议拆。
    assert!(joined
        .contains("tc class replace dev eth0 parent 1: classid 1:101 htb rate 25mbit ceil 25mbit"));
    assert!(joined.contains(
        "tc filter replace dev eth0 parent 1: protocol ip prio 101 handle 65537 fw flowid 1:101"
    ));
    assert!(joined.contains("tc qdisc replace dev ifb-xrayc root handle 2: htb default 1"));
    assert!(joined.contains(
        "tc class replace dev ifb-xrayc parent 2: classid 2:102 htb rate 50mbit ceil 50mbit"
    ));
    assert!(joined.contains(
        "iptables -t mangle -A XRAYC_LIMITER_OUTPUT -m mark --mark 65537 -j CONNMARK --save-mark"
    ));
    // 绝不为 TCP/UDP 拆单独 tc 规则:命令里不得出现任何按协议匹配的痕迹。
    assert!(!joined.contains("protocol tcp"));
    assert!(!joined.contains("--protocol tcp"));
    assert!(!joined.contains("-p tcp"));
}

#[test]
fn test_limiter_command_plan_stress_scales_to_many_directional_users() {
    // 限速压测:大量方向限速用户时,命令计划生成耗时与命令数随用户线性增长且正确。
    use std::time::Instant;
    let settings = LimiterPlanSettings {
        enabled: true,
        interface: "eth0".to_string(),
        ifb_interface: "ifb-xrayc".to_string(),
        root_rate_bps: 10_000_000_000,
        connmark_available: true,
    };
    let n: u16 = 2000;
    let users: Vec<UserRateLimit> = (0..n)
        .map(|i| UserRateLimit {
            user_id: format!("user-{i}"),
            xray_user_key: format!("u-{i}@xrayc.local"),
            rate_limit_bps: 10_000_000,
            rate_limit_up_bps: 2_000_000,    // 上行 2mbit
            rate_limit_down_bps: 10_000_000, // 下行 10mbit
            mark: 0x10000 + u32::from(i) + 1,
            class_id: 101 + i,
        })
        .collect();
    let start = Instant::now();
    let commands = build_limiter_command_plan(&settings, &users);
    let elapsed = start.elapsed();
    eprintln!(
        "[限速压测] {n} 个方向限速用户 → {} 条 tc/iptables 命令,生成耗时 {:?}",
        commands.len(),
        elapsed
    );
    let joined = commands.join("\n");
    let last = 101 + (n - 1);
    // 抽查第 n 个用户:上行类(eth0 egress)用 2mbit、下行类(ifb)用 10mbit,规模下方向限速仍正确。
    assert!(
        joined.contains(&format!(
            "tc class replace dev eth0 parent 1: classid 1:{last} htb rate 2mbit ceil 2mbit"
        )),
        "第 {n} 个用户上行类(eth0)应正确"
    );
    assert!(
        joined.contains(&format!(
            "tc class replace dev ifb-xrayc parent 2: classid 2:{last} htb rate 10mbit ceil 10mbit"
        )),
        "第 {n} 个用户下行类(ifb)应正确"
    );
    // 命令数随用户线性增长(每用户至少 上行类+filter、下行类+filter 4 条)。
    assert!(
        commands.len() > usize::from(n) * 4,
        "命令数 {} 应随用户线性增长",
        commands.len()
    );
    // 大量用户的纯命令生成应很快(通常几十毫秒级),给宽松上限防退化。
    assert!(
        elapsed.as_millis() < 3000,
        "{n} 用户 plan 生成应 < 3s,实际 {elapsed:?}"
    );
}

#[test]
fn test_limiter_command_plan_applies_asymmetric_up_down_rates() {
    // 上下行分开限速:上行类 1:N(eth0 egress)用上行速率,下行类 2:N(ifb)用下行速率。
    let settings = LimiterPlanSettings {
        enabled: true,
        interface: "eth0".to_string(),
        ifb_interface: "ifb-xrayc".to_string(),
        root_rate_bps: 10_000_000_000,
        connmark_available: true,
    };
    let users = vec![UserRateLimit {
        user_id: "user-a".to_string(),
        xray_user_key: "u-a@xrayc.local".to_string(),
        rate_limit_bps: 10_000_000,
        rate_limit_up_bps: 2_000_000,
        rate_limit_down_bps: 10_000_000,
        mark: 65_537,
        class_id: 101,
    }];
    let joined = build_limiter_command_plan(&settings, &users).join("\n");
    assert!(
        joined.contains(
            "tc class replace dev eth0 parent 1: classid 1:101 htb rate 2mbit ceil 2mbit"
        ),
        "上行类 1:101(eth0 egress)应用上行速率 2mbit\n{joined}"
    );
    assert!(
        joined.contains(
            "tc class replace dev ifb-xrayc parent 2: classid 2:101 htb rate 10mbit ceil 10mbit"
        ),
        "下行类 2:101(ifb)应用下行速率 10mbit\n{joined}"
    );
}

/// 钉死下行(下载回程)整形:必须存在按连接标记把全部回程流量
/// redirect 到 IFB 的 matchall+connmark 规则,且依赖 act_connmark 内核模块。
/// 该规则缺失 = 下载不限速 = 计费红线 bug 回归。
#[test]
fn test_limiter_command_plan_shapes_download_return_path_via_connmark_redirect() {
    let settings = LimiterPlanSettings {
        enabled: true,
        interface: "eth0".to_string(),
        ifb_interface: "ifb-xrayc".to_string(),
        root_rate_bps: 10_000_000_000,
        connmark_available: true,
    };
    let users = vec![UserRateLimit {
        user_id: "user-a".to_string(),
        xray_user_key: "u-a@xrayc.local".to_string(),
        rate_limit_bps: 1_000_000,
        rate_limit_up_bps: 1_000_000,
        rate_limit_down_bps: 1_000_000,
        mark: 65_537,
        class_id: 101,
    }];

    let joined = build_limiter_command_plan(&settings, &users).join("\n");

    // 内核模块:tc 层读取 conntrack 标记需要 act_connmark。
    assert!(joined.contains("modprobe act_connmark || true"));
    // 双向闭合:回程包按 conntrack 标记整体 redirect 到 IFB,
    // 再由 IFB 上 per-user `fw` 过滤器分流到各自 HTB class。
    assert!(joined.contains(
        "tc filter add dev eth0 parent ffff: protocol ip prio 32766 matchall action connmark action mirred egress redirect dev ifb-xrayc"
    ));
    // 回程整形依赖 conntrack 标记的 save:出站 SO_MARK 存进 conntrack,
    // 回程经 eth0 ingress 的 act_connmark 读回 skb。
    assert!(joined.contains(
        "iptables -t mangle -A XRAYC_LIMITER_OUTPUT -m mark --mark 65537 -j CONNMARK --save-mark"
    ));
    // per-user 下行区分:IFB 上每用户独立 class + fw 过滤器。
    assert!(joined.contains(
        "tc filter replace dev ifb-xrayc parent 2: protocol ip prio 101 handle 65537 fw flowid 2:101"
    ));
}

/// 钉死内核 act_connmark 不可用时的限速优雅降级(§7.7.1):
/// 1) 跳过下行 connmark/IFB 整形(matchall+connmark redirect、act_connmark modprobe、
///    CONNMARK save、IFB 上的下行 class/filter);
/// 2) 保留上行 eth0 egress 按 fwmark 整形(不依赖 connmark);
/// 3) 计划仍正常生成、不空、带降级标记注释(对应运行时不 bail 整次 apply)。
#[test]
fn test_limiter_command_plan_degrades_gracefully_when_connmark_unavailable() {
    let settings = LimiterPlanSettings {
        enabled: true,
        interface: "eth0".to_string(),
        ifb_interface: "ifb-xrayc".to_string(),
        root_rate_bps: 10_000_000_000,
        // 内核能力降级:connmark 不可用。
        connmark_available: false,
    };
    let users = vec![UserRateLimit {
        user_id: "user-a".to_string(),
        xray_user_key: "u-a@xrayc.local".to_string(),
        rate_limit_bps: 1_000_000,
        rate_limit_up_bps: 1_000_000,
        rate_limit_down_bps: 1_000_000,
        mark: 65_537,
        class_id: 101,
    }];

    let commands = build_limiter_command_plan(&settings, &users);
    let joined = commands.join("\n");

    // 下行 connmark/IFB 整形整体跳过:相关命令都不得出现。
    assert!(
        !joined.contains("action connmark"),
        "降级时不得生成下行 connmark matchall redirect 规则"
    );
    assert!(
        !joined.contains("modprobe act_connmark"),
        "降级时不得 modprobe act_connmark"
    );
    assert!(
        !joined.contains("CONNMARK --save-mark"),
        "降级时不得生成 CONNMARK save-mark"
    );
    assert!(
        !joined.contains("redirect dev ifb-xrayc"),
        "降级时不得把流量 redirect 进 IFB"
    );
    assert!(
        !joined.contains("dev ifb-xrayc parent 2:"),
        "降级时不得在 IFB 上建下行 class/filter"
    );

    // 上行 eth0 egress 按 fwmark 整形必须保留(不依赖 connmark)。
    assert!(
        joined.contains(
            "tc filter replace dev eth0 parent 1: protocol ip prio 101 handle 65537 fw flowid 1:101"
        ),
        "降级时上行 eth0 egress fwmark 整形必须保留"
    );
    assert!(joined
        .contains("tc class replace dev eth0 parent 1: classid 1:101 htb rate 1mbit ceil 1mbit"));
    // 计划非空且带降级标记注释:对应运行时整次 apply 不 bail。
    assert!(joined.contains("xrayc limiter degraded"));
    assert!(commands.len() > 3);
}

/// 反向确认:connmark 可用路径行为零变化——下行整形规则齐全,无降级标记。
#[test]
fn test_limiter_command_plan_keeps_download_shaping_when_connmark_available() {
    let settings = LimiterPlanSettings {
        enabled: true,
        interface: "eth0".to_string(),
        ifb_interface: "ifb-xrayc".to_string(),
        root_rate_bps: 10_000_000_000,
        connmark_available: true,
    };
    let users = vec![UserRateLimit {
        user_id: "user-a".to_string(),
        xray_user_key: "u-a@xrayc.local".to_string(),
        rate_limit_bps: 1_000_000,
        rate_limit_up_bps: 1_000_000,
        rate_limit_down_bps: 1_000_000,
        mark: 65_537,
        class_id: 101,
    }];

    let joined = build_limiter_command_plan(&settings, &users).join("\n");

    // 内核可用:下行整形语义齐全。
    assert!(joined.contains("modprobe act_connmark || true"));
    assert!(joined.contains(
        "tc filter add dev eth0 parent ffff: protocol ip prio 32766 matchall action connmark action mirred egress redirect dev ifb-xrayc"
    ));
    assert!(joined.contains(
        "iptables -t mangle -A XRAYC_LIMITER_OUTPUT -m mark --mark 65537 -j CONNMARK --save-mark"
    ));
    // 无降级标记注释。
    assert!(!joined.contains("xrayc limiter degraded"));
}

#[test]
fn shaping_present_in_qdisc_output_detects_htb_root_by_handle() {
    // 心跳级自愈的探测判据:`tc qdisc show dev X` 输出含对应 handle 的 htb 根 = 整形在。
    // 上行根 htb 1:、下行(IFB)根 htb 2:。
    let up = "qdisc htb 1: root refcnt 2 r2q 10 default 1 direct_packets_stat 0 direct_qlen 32";
    assert!(shaping_present_in_qdisc_output(up, "1"));
    let ifb = "qdisc htb 2: root refcnt 2 r2q 10 default 0x1";
    assert!(shaping_present_in_qdisc_output(ifb, "2"));
    // 被 `tc qdisc del dev X root` 拆掉后:回落默认 qdisc、不含 htb 1: → 判缺失(触发自愈重建)。
    let torn = "qdisc noqueue 0: root refcnt 2";
    assert!(!shaping_present_in_qdisc_output(torn, "1"));
    let torn2 = "qdisc mq 0: root\nqdisc fq_codel 0: parent :1";
    assert!(!shaping_present_in_qdisc_output(torn2, "1"));
    // 边界:handle 精确匹配,htb 11: 不得被误判成 htb 1:。
    assert!(!shaping_present_in_qdisc_output(
        "qdisc htb 11: root refcnt 2",
        "1"
    ));
}
